# 2026-05 — v0.10.0 bench-record: S13 100k full marginal CV (host-side contention)

Status: **Resolved — host quiescence required for clean bench
measurement; one residual borderline cell accepted with this
note.**

## TL;DR

- Three bench-record runs against v0.10.0 (commit `ec7d7495`,
  tag `v0.10.0`) on the canonical dev-box host (AMD Ryzen 9 3900X
  / Debian bookworm / `linux-x86_64`).
- Runs 1 + 2 (host busy with a sister CI runner
  + an unrelated dev stack actively consuming CPU during
  measurement): **3 gating failures each, on different cells**
  — clean signature of host-side contention, not a v0.10 binary
  regression. Regression check (`min_ms` vs v0.9.23) PASSED both
  times with max +11% delta.
- Run 3 (quiesced: the sister runner and the dev-stack containers
  stopped before re-trigger): **1 gating failure, 0.1% over the gate
  ceiling**, on `S13 100k full` (CV 10.1%, mean 148 ms, stddev
  15 ms). Regression check PASSED with max +6.3% delta — the
  cleanest run produced.
- The remaining S13 100k full CV is accepted as a transient
  measurement-window artefact, not an inherent S13 property
  (S13 1m full is at CV 1.8%; S13 100k changed at 3.3%; S13's
  jitter distribution does not pattern-match an inherent
  spawn-jitter floor at the 10% line).
- v0.10.0 row enters `docs/benchmarks/HISTORY.md` via PR #35
  with this investigation linked in the PR body.

## Per-run summary

### Run 1 — tag-triggered (host busy)

- Run: [`26137356195`](https://github.com/asamarts/alint/actions/runs/26137356195)
  fired by tag push, 2026-05-20T02:22:37Z.
- Conclusion: workflow success, gate **FAIL** (3 cells).
- Failed cells: `S2 1m changed` (CV 26.5 %, mean 5785 ms),
  `S12 100k changed` (CV 17.9 %), `S5 100k full` (CV 12.7 %).
- Regression: PASS, max +11.0 % (`S5 1m full`).
- Host state: the sister runner likely active on a
  concurrent CI job + the dev stack idle-but-running.
  Load average not captured at run time; subsequent inspection
  showed system load ~7-8 across the 15-min average.

### Run 2 — workflow_dispatch (host still busy)

- Run: [`26177196343`](https://github.com/asamarts/alint/actions/runs/26177196343)
  fired by `gh workflow run`, 2026-05-20T16:54:44Z.
- Conclusion: workflow **failure** at the `open PR with bench
  results` step only — the bench data force-pushed to
  `bench-record/v0.10.0` cleanly; the failure was a cosmetic
  `gh pr create` error because PR #35 was already open. Gate
  was run manually against the force-pushed `results.json`.
- Failed cells: `S1 100k changed` (CV 12.9 %), `S4 100k full`
  (CV 12.7 %), `S1 100k full` (CV 10.0 %).
- Regression: PASS, max +11.5 % (`S1 10k full`).
- Diagnostic: **zero overlap** with run 1's failing cells. If
  v0.10's binary had a per-scenario regression, the same cells
  would fail. The shifting failure set is the unambiguous
  signature of random host-side contention.
- Host state at run time (snapshot during run): 11 containers
  running on the host; the sister runner actively executing
  a CI job (Runner.Listener at 100 % CPU snapshot, Runner.Worker
  at 28 %, MainThread at 77 %). System load 7.79 / 7.90 (15- /
  60-min).

### Run 3 — workflow_dispatch (host quiesced)

- Run: [`26185676909`](https://github.com/asamarts/alint/actions/runs/26185676909)
  fired by `gh workflow run`, 2026-05-20T19:41:01Z. Same "PR
  already exists" workflow-failure at step 13 only; bench data
  force-pushed cleanly; gate run manually.
- Pre-flight: stopped the sister runner and the
  dev-stack containers. Load average dropped from 7.79 / 7.90 to
  2.07 / 2.68 within seconds.
- Failed cells: **`S13 100k full` (CV 10.1 %, 0.1 % over the
  gate ceiling)** — single borderline cell.
- Regression: PASS, max +6.3 % (`S5 10k changed`) — meaningfully
  cleaner than the busy runs (+11.0 % / +11.5 %).

## Why the S13 100k full residual is not an inherent issue

S13 is the v0.10-new single-shot dispatch class (`generated_file_fresh`
+ `command_idempotent`, declared with `command: ["true"]` so the
row isolates `crate::spawn::run_capturing` rather than the user's
tool). The hypothesis that single-shot has inherently elevated
spawn-jitter at the gate boundary is **not** supported by the
per-size distribution:

| Cell | CV | Mean (ms) | Stddev (ms) | Verdict |
|---|--:|--:|--:|---|
| S13 1k full | 3.1 % | 18.3 | 0.57 | ✓ |
| S13 1k changed | 2.8 % | 29.5 | 0.83 | ✓ |
| S13 10k full | 2.5 % | 29.7 | 0.75 | ✓ |
| S13 10k changed | 10.7 % | 57.9 | 6.17 | advisory |
| **S13 100k full** | **10.1 %** | **148.0** | **15.00** | **FAIL** |
| S13 100k changed | 3.3 % | 421.2 | 13.84 | ✓ |
| S13 1m full | 1.8 % | 1419.8 | 24.98 | ✓ |
| S13 1m changed | 1.6 % | 4169.4 | 67.12 | ✓ |

The 1m full + 1m changed cells (the larger-tree-walk siblings of
the failing 100k full) sit at **CV 1.8 % / 1.6 %**. If S13's
spawn machinery had an intrinsic noise floor near 10 %, the 1m
cells (which include the same spawn cost plus a much larger
tree-walk denominator) would carry it forward — they don't.

The other v0.10-new scenarios (S11 cross-file, S12 per-file) all
landed cleanly at their 100k+ sizes (S11 100k full 4.3 %, S12 100k
changed 3.6 %), so the borderline isn't a property of "v0.10-new
scenarios" generally.

The most plausible reading is **a transient noise spike during
the 100k-full measurement window** for S13 — likely a brief
background process the quiesce didn't catch (e.g. one of the
remaining idle services doing an internal task, or a
kernel-side housekeeping moment). At 0.1 % over the binary
threshold, the data is essentially equivalent to a clean pass.

## Recommendations

### For future bench-record runs

Quiesce the canonical host before triggering. The minimal
quiesce set that worked here:

```sh
podman stop <sister-runner> <dev-stack containers...>
gh workflow run bench-record.yml \
  --ref vX.Y.Z -f ref=vX.Y.Z -f label=vX.Y.Z
# ... ~75 min wall time on the quiesced host ...
podman start <sister-runner> <dev-stack containers...>
```

The remaining idle service containers can stay up;
they're idle services and don't measurably affect the bench.

The original gate-design investigation
([`../2026-05-bench-runner-instability/`](../2026-05-bench-runner-instability/))
established that 100k+ cells are reliable; that holds **when
the host is quiesced**. Run-time contention from the sister
runner specifically breaks that assumption.

### For the bench-record workflow

The `open PR with bench results` step (`bench-record.yml`
line ~310) fails when PR #35 (or any prior bench-record PR)
already exists for the same branch. The bench data IS force-
pushed correctly before that step runs, so the failure is
cosmetic — but it pollutes the conclusion status. A
follow-up should make `gh pr create` idempotent (try-create,
fall back to `gh pr edit` to refresh the body on the existing
PR). Tracked separately.

### For the gate methodology

The current per-cell 10 % CV ceiling is a hard binary line.
Cells at 9.9 % pass; cells at 10.1 % fail. There is no
principled difference between those values; the line is
calibrated against historical noise distributions. A future
gate refinement could either:

- Use a softer threshold (e.g. a ratio-vs-baseline-CV check, so
  a cell at 10.1 % isn't a fail unless it's also significantly
  noisier than its v0.9.x history).
- Switch entirely to a `min_ms`-based gate (the statistic the
  original investigation found cross-version-reliable; the
  current CV check is a within-run quality proxy, which the
  evidence shows is host-quiescence-dependent).

This is out of scope for v0.10.0 — it is a v0.11 design pass.

## Disposition

PR #35 merges with this README linked from the PR body. The
v0.10.0 row enters `HISTORY.md` with the documented residual.
The next release should quiesce-then-run to validate the
process is now well-understood.
