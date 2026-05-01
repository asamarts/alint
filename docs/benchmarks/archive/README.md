# Benchmark archive

Superseded snapshots, kept for cross-version diffs. **Do not edit.**

## What's here

### `v0.1-linux-x86_64.md`

Single-file v0.1 snapshot from when the bench layout was one file per
release. Predates the criterion + hyperfine split; numbers aren't
comparable to anything later because the harness was different. Kept
for completeness.

### `v0.9-development-baselines/`

Per-phase criterion baselines from the v0.9 development cut. Each
sub-directory was the "before" reference for a v0.9.x phase's
`bench-compare` gate:

- `baseline-pre/criterion/` — frozen at the v0.9 starting commit
  `bec0cf4`, before any phase landed. Used by v0.9.1 to measure the
  walker delta.
- `baseline-v0.9.1/criterion/` — frozen after v0.9.1 (parallel
  walker), before v0.9.2 (memory pass).
- `baseline-v0.9.2/criterion/` — frozen after v0.9.2, before v0.9.3
  (dispatch flip).
- `baseline-v0.9.3/criterion/` — frozen after v0.9.3, before v0.9.4
  (content-rule mechanical migration).

These were never the published numbers — the published numbers for
each tagged release live under
[`../micro/results/linux-x86_64/<version>/`](../micro/results/linux-x86_64/).
The baselines exist because each v0.9.x phase's PR ran
`bench-compare --before <prior-baseline> --after target/criterion` to
gate against the prior phase rather than the v0.7.0 floor.

### `v0.9-development-phases/`

Per-phase criterion *output* (the "after" of each `bench-compare`
during the v0.9 development cut). One sub-directory per phase:

- `v0.9.1-parallel-walker/criterion/`
- `v0.9.2-memory-pass/criterion/`
- `v0.9.3-dispatch-flip/criterion/`
- `v0.9.4-content-rules/criterion/` — the v0.9.4 published numbers
  (also live under `micro/results/linux-x86_64/v0.9.4/`; this copy
  is intentional redundancy for narrative continuity in
  `docs/design/v0.9/`).

Each phase's README is the original write-up captured at the time of
the phase's PR; the per-bench delta tables in those READMEs are the
authoritative record of what each phase did vs the previous one.

## Why kept, not deleted

Three reasons:

1. **Cross-version diffs.** A future engineer asking "did the
   walker get slower at 10k between v0.9.1 and v0.9.2?" can run
   `xtask bench-compare --before archive/v0.9-development-baselines/baseline-v0.9.1/criterion --after archive/v0.9-development-baselines/baseline-v0.9.2/criterion` directly. Without the archive,
   answering that question requires checking out two old commits and
   re-running benches on whatever current hardware is.
2. **Investigation provenance.** The v0.9.5 cliff investigation
   (under [`../investigations/2026-05-cross-file-rules/`](../investigations/2026-05-cross-file-rules/)) cross-references these
   per-phase numbers when discussing what each v0.9.x phase moved.
   Deleting them breaks those links.
3. **Cheap.** The criterion-format directories are small; total
   archive footprint is tens of MB across all of v0.9-development-*.

## Reading these vs the published corpus

The split:

- **Published numbers** (`micro/results/<arch>/<version>/`,
  `macro/results/<arch>/<version>/`) — the official record for each
  release tag. Comparable cross-release.
- **Archive** (here) — intermediate snapshots that informed
  development-cycle gating. Comparable within a development cut, not
  cross-release.

If you're looking for "what is alint's current speed?", read
[`../README.md`](../README.md) and the latest published numbers
under `micro/` / `macro/`. The archive is for deeper history.
