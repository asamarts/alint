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
