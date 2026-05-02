# v0.9.8 — Cross-file dispatch fast paths, round 2

Status: Design draft, written 2026-05-02 after the v0.9.7 patch
release shipped (and after the v0.9.5/v0.9.6/v0.9.7 macro-bench
backfill surfaced the S7-1M cliff that this cut targets).

## What v0.9.8 ships

A second round of cross-file dispatch fast paths, this time
targeting the four kinds the v0.9.5 `for_each_dir` fix didn't
cover. Same shape as v0.9.5 — a lazy `OnceLock` index on
`FileIndex` that collapses an O(D × N) scan into O(N + Σ|children|)
amortised — extended to direct-child enumeration so `dir_only_contains`,
`dir_contains`, `every_matching_has`, and the nested-rule paths
under `for_each_dir` / `for_each_file` all stop scanning the full
entries vec per dispatched dir.

| File | Sub-theme |
|---|---|
| [`cross-file-fast-paths-v2.md`](./cross-file-fast-paths-v2.md) | Engine-internal: extend `FileIndex` with `parent_to_children` (lazy `HashMap<Arc<Path>, Vec<usize>>`), `file_basenames_of`, `descendants_of`. Refactor 5 cross-file rules to consume the new index. |

## Headline numbers (target)

The 1M S7 cell at v0.9.5 measured **652.43 s ± 50.35** — the
cliff this cut exists to fix. v0.9.6 / v0.9.7 carry the same
shape (no cross-file dispatch changes between the three).
Acceptance gate for v0.9.8:

- 1M S7 full **drops below 100 s** (≥ 6.5× speedup).
- 1M S6 (per-file content fan-out, currently fast at v0.9.5)
  stays within ±5 % — no regression on the per-file dispatch
  path the v0.9.3 dispatch flip optimised.
- All other 1M cells (S1–S5, S8, S9) within ±5 % of the v0.9.7
  baseline.

## Why this and not v0.10 (LSP)?

v0.10 stays reserved for the LSP server cut per
[`docs/design/v0.10/README.md`](../v0.10/README.md). v0.9.8
finishes the v0.9 perf cut (which v0.9.5 started and v0.9.6
extended with `scope_filter`). The LSP single-file re-evaluation
contract directly benefits from a fully O(1) cross-file dispatch
shape — a v0.9.8 → v0.10 ordering means the LSP server inherits
a clean perf floor instead of papering over the cliff.

## Cross-cutting decisions

### Index granularity

Three new accessors on `FileIndex`, each backed by its own
`OnceLock`:

1. **`children_of(dir)` → `&[usize]`** — direct file + subdir
   children of `dir`, by index into `entries`. The primary fix
   for `dir_only_contains` and `dir_contains`.
2. **`file_basenames_of(dir)` → `&[&str]`** — direct file
   children's basenames, pre-extracted. Skips per-call
   `file_name().to_str()` in `dir_contains`'s matcher loop.
3. **`descendants_of(dir)` → `impl Iterator<Item = &FileEntry>`**
   — recursive. Uses `children_of` internally; does NOT
   materialise the full subtree as a Vec (would be O(N) memory
   for the root dir at 1M files). Yields entries depth-first.

`children_of` is the workhorse — `file_basenames_of` and
`descendants_of` are derived helpers. All three use the same
lazy-build pattern as the v0.9.5 `path_set`: `OnceLock` field,
build on first call, share across all subsequent lookups.

### Tracing instrumentation policy

Per the v0.9.7 lessons (the `phase!` macro's `#[allow(clippy::cast_possible_truncation)]`
and the user-visible `ALINT_LOG` env var), the existing engine
phase events fire in release builds — `tracing::info!` checks
the global level filter at runtime, so when no subscriber matches
the cost is one filter probe.

The new index-build events are **debug-only**: gated behind
`#[cfg(debug_assertions)]` so they're compiled out entirely in
release builds. Rationale: index builds happen at most three
times per `alint check` invocation (once per accessor first-use)
and contribute nothing actionable to a normal user's debug
session. The existing `engine.phase` events (which fire per
*rule*) are the right surface for users; the new index-build
events are for `xtask bench-scale` profile runs and for
contributor debugging — both of which can use a debug build.

```rust
// crates/alint-core/src/walker.rs
#[cfg(debug_assertions)]
macro_rules! trace_index_build {
    ($kind:expr, $start:expr, $entries:expr) => {
        tracing::debug!(
            phase = "index_build",
            kind = $kind,
            elapsed_us = u64::try_from($start.elapsed().as_micros())
                .unwrap_or(u64::MAX),
            entries = $entries as u64,
            "engine.index",
        );
    };
}
#[cfg(not(debug_assertions))]
macro_rules! trace_index_build {
    ($kind:expr, $start:expr, $entries:expr) => {};
}
```

A `#[cfg(debug_assertions)]` macro sites the timer and event
emission together, so release builds compile away the `Instant::now()`
call too — true zero-overhead.

### Behavioural invariants the engine preserves

Three things v0.9.8 must not change:

1. **Snapshot-stable output across runs.** The new accessors are
   read-only views over the existing `entries` vec; the index
   builds are deterministic functions of `entries`. No new
   non-determinism source. Existing per-formatter sorts continue
   to apply.
2. **Public `FileIndex` surface remains add-only.** The three
   new accessors are additive. Existing methods (`files()`,
   `dirs()`, `contains_file`, `find_file`, `entries`,
   `file_path_set`) keep their signatures and semantics.
3. **Rule semantics preserved.** Each refactored rule's e2e
   scenarios stay green; new property tests assert old-shape
   vs new-shape produce identical violations on 1k-file inputs.

### Memory cost

`parent_to_children` adds a `HashMap<Arc<Path>, Vec<usize>>`
keyed by every directory in the index. At 1M files in a 5K-dir
monorepo, that's ~5K HashMap entries × (pointer + Vec header) ≈
500 KB. Vec contents total ≈ 1M × 8 bytes (usize index) = 8 MB.
Negligible compared to the 7.9 GB the 1M tree itself occupies on
disk and the ~1 GB the `entries` vec occupies in-memory.

`file_basenames_of` adds `~5K × Vec<&str>` headers + per-file
`&str` (16 bytes each = 16 MB at 1M). Borrowed from
`entries[i].path`; no allocation.

### Schema versioning

No `.alint.yml` schema changes. Every v0.9.7 config runs
unchanged on v0.9.8.

## Implementation order

1. **Phase A** — `tracing` profile of v0.9.7 1M S7 to confirm
   the dispatch shape before touching code. ~30 min.
2. **Phase B** — Add `parent_to_children` + helpers to
   `FileIndex`. Land as a standalone commit; no rule changes
   yet, so e2e and bench numbers stay identical. ~3 hr.
3. **Phase C** — Refactor `dir_only_contains` (commit 1),
   `dir_contains` (commit 2), audit `for_each_dir` /
   `for_each_file` / `every_matching_has` (commit 3). One
   commit per rule so `git bisect` localises any regression.
   ~5 hr.
4. **Phase D** — `coverage_audit_cross_file_dispatch.rs` +
   proptest A/B + new e2e scenarios. ~2 hr.
5. **Phase E** — Bench gate against v0.9.7 baseline. ~3 hr.
   If gate misses, iterate on Phase C.
6. **Phase F** — Release v0.9.8. Tag + push fires release.yml +
   bench-record.yml; bench-record opens the publish-grade PR.
   ~30 min.

Total: ~14 hours engineering + ~3 hours bench wall-time.

## Out of scope for v0.9.8

Explicitly held back to keep the cut tight:

- **`unique_by` HashMap-key tuning.** v0.9.5 + the existing
  HashMap-based dedup is already O(N); the cliff isn't here.
- **Cross-file rule parallelism.** The cross-file partition runs
  rule-major par-iter; refactoring to file-major would touch the
  dispatch flip (v0.9.3) and is larger than v0.9.8 scope.
- **`for_each_dir` nested-rule build cache.** Each iteration
  re-builds the nested rule from the spec (~µs at our scale).
  Caching the built rule per (parent_id, idx) shape would shave
  some, but the savings are dominated by the per-iteration eval
  cost the children_of fix already addresses.
- **LSP integration.** v0.10 picks up where v0.9.8 leaves off
  with the per-file dispatch shape; the `Engine::run_for_file`
  contract there benefits from the cross-file fast paths but
  doesn't drive their design.

## How to use this doc

Same shape as the v0.7 / v0.9 / v0.10 design passes. The single
sub-doc `cross-file-fast-paths-v2.md` carries the
Problem / Surface / Semantics / FP-surface / Implementation /
Tests / Open-questions sections.

When implementation starts, Status header on the sub-doc flips
to "Implemented in v0.9.8" with the commit pointer; open
questions get resolved in the doc itself, mirroring the v0.7
`git_blame_age.md` "Resolved open questions" block at the top.
