# Parallel walker

Status: Design draft.

## Problem

`crates/alint-core/src/walker.rs:94` consumes a sequential
`WalkBuilder::build()` iterator: one thread crawls every
directory, computes metadata, and pushes a `FileEntry` onto a
`Vec`. At small repo sizes this is fine; at the v0.8.4
hyperfine S5 baseline (10k synthetic tree, 4 threads available)
the walk is ~30% of total `alint check` time, with three of
four cores idle while the walker thread is iterating.

The `ignore` crate already exposes a parallel visitor —
`WalkBuilder::build_parallel()` returns a `WalkParallel` whose
`.run()` method spawns N worker threads and dispatches a
user-supplied closure per directory entry. Switching to it is
the single biggest walker speedup available without rewriting
the walker layer.

The catch: `WalkBuilder::build()` produces entries in a
deterministic order (depth-first, alphabetical within a
directory). `WalkBuilder::build_parallel()` produces them in
worker-scheduling order — non-deterministic across runs, across
hosts, and across thread counts. Snapshot tests, e2e fixtures,
and the markdown / human formatters all currently assume
walker output is stable enough that two runs produce the same
violations in the same order.

The mitigation is a deterministic post-sort by relative path
inside `walk()` itself, before the `FileIndex` is returned.
Walker callers see identical output to today; only the
intermediate parallelism is new.

## Surface area

Internal to `crates/alint-core/src/walker.rs`. The public
surface — the `walk(root, &WalkOptions) -> Result<FileIndex>`
free function — is unchanged. `FileIndex { entries: Vec<FileEntry> }`
is unchanged. `FileEntry { path, is_dir, size }` is unchanged.

A new top-level config field is **not** added in v0.9.1. The
`ignore` crate's `WalkBuilder::threads(n)` defaults to
`num_cpus::get()`, which is the right default for almost every
adopter. If a CI runner with a thread budget needs to bound the
walker, the user can set `RAYON_NUM_THREADS` to bound the rule
engine; the walker thread count is independent (see "Open
questions").

## Semantics

For each call to `walk(root, opts)`:

1. Build the same `WalkBuilder` we build today (gitignore
   handling, `.git/` exclusion, `extra_ignores` overrides —
   identical to lines `walker.rs:60..91`).
2. Call `builder.build_parallel()` instead of `builder.build()`.
3. Each worker thread accumulates `FileEntry`s into a thread-
   local `Vec`. Avoid a shared `Mutex<Vec<FileEntry>>` — that
   re-serialises everything we just parallelised.
4. The visitor closure has the same per-entry logic as today:
   strip the root prefix, skip empty rels, fetch metadata,
   build the `FileEntry`. Errors propagate via a shared
   `parking_lot::Mutex<Option<Error>>` (see "Open questions"
   on fail-fast vs. collect-all).
5. After `.run()` returns, flatten the per-thread `Vec`s into
   the final `entries: Vec<FileEntry>`.
6. Sort:

   ```rust
   entries.sort_unstable_by(|a, b| a.path.cmp(&b.path));
   ```

   `sort_unstable` because path comparison is total — no
   tie-breaking needed.
7. Return `FileIndex { entries }`.

The post-sort restores byte-identical `FileIndex` output to the
sequential walker for any tree where filenames are unique
(which is everything `ignore` produces — every entry has a
unique relative path).

## Behavioural invariants

- `walk()` output is **byte-identical** between v0.8.2 and
  v0.9.1 for any input tree. Diffing the v0.8.5 e2e snapshot
  outputs against v0.9.1 should produce zero diff.
- `walk()` errors fail-fast on the first error, same as today.
  The first thread to encounter an `entry.metadata()` failure or
  an unstrippable prefix sets the shared error slot; the visitor
  bails out (`WalkState::Quit`); subsequent threads short-circuit;
  `walk()` returns the captured error.
- Thread count defaults to `num_cpus::get()`. Tests that need
  determinism for parallel-execution edge cases can set
  `WalkBuilder::threads(1)` via a private knob (we don't expose
  it in `WalkOptions` for v0.9.1).

## False-positive surface

(For an internal change, this section catalogues bugs the new
shape can introduce.)

- **Latent ordering assumption in a rule.** A rule that today
  iterates `ctx.index.files()` and assumes it sees entries in
  filesystem-traversal order would silently change behaviour
  even with the post-sort, because the new order is alphabetical-
  by-path and the old order is depth-first. Mitigation: grep for
  any rule that constructs state across iterations
  (`for_each_dir` builds nested rules; `unique_by` accumulates a
  multimap; `dir_only_contains` walks the index for each dir).
  None of those rely on traversal order — they sort their own
  outputs internally — but the audit goes in the v0.9.1 PR.
- **Non-deterministic test failure if post-sort is forgotten.**
  Easy mistake to introduce later. Mitigation: add a `walker.rs`
  unit test that runs `walk(...)` twice on the same tree and
  asserts the two `FileIndex` outputs are equal. CI catches a
  forgotten sort on the next regression.
- **Worker-thread `entry.metadata()` syscall surfaces races on
  filesystems with active churn.** The sequential walker reads
  metadata one entry at a time, so a file deleted mid-walk
  produces one error. A parallel walker reads K entries at a
  time, so the same race produces K errors. Today this is
  treated as fail-fast: any error aborts the walk. Behaviour is
  unchanged — the user still gets one error and a non-zero exit
  code — but the *which* error they get is non-deterministic
  across runs. Acceptable: the user's repo is unstable;
  alint's job is to report it, not paper over it.
- **`build_parallel()` swallows panics from the closure.** The
  `ignore` crate documents that closure panics propagate to
  `.run()`'s caller via `WalkState::Quit` semantics, but the
  implementation can be quirky. Mitigation: the closure's body
  has no panicking branches today (we use `Result`-returning
  helpers everywhere); the audit confirms that.

## Implementation notes

**Crate location:** `crates/alint-core/src/walker.rs`. No new
files. No new dependencies (the `ignore` crate already exposes
`build_parallel` from the same `WalkBuilder` we use today).

**Dependency note:** `parking_lot` is *not* a dep of
`alint-core` today (`std::sync::Mutex` is fine for a single
write under contention; we hold the lock once per error, not
per entry). Use `std::sync::Mutex<Option<Error>>` and skip
adding `parking_lot`.

**Sketch:**

```rust
pub fn walk(root: &Path, opts: &WalkOptions) -> Result<FileIndex> {
    let builder = build_walk_builder(root, opts)?;          // unchanged from today

    use std::sync::Mutex;
    let entries: Mutex<Vec<Vec<FileEntry>>> = Mutex::new(Vec::new());
    let first_error: Mutex<Option<Error>> = Mutex::new(None);

    builder.build_parallel().run(|| {
        let entries_handle = &entries;
        let error_handle = &first_error;
        let mut local: Vec<FileEntry> = Vec::new();
        Box::new(move |result| {
            // Cheap exit if another thread already failed.
            if error_handle.lock().unwrap().is_some() {
                // Push our local accumulation before bailing so
                // we don't drop work that already succeeded; the
                // caller discards it on error anyway.
                entries_handle.lock().unwrap().push(std::mem::take(&mut local));
                return ignore::WalkState::Quit;
            }
            match result_to_entry(root, result) {
                Ok(Some(e)) => {
                    local.push(e);
                    ignore::WalkState::Continue
                }
                Ok(None) => ignore::WalkState::Continue, // root entry / non-strippable
                Err(e) => {
                    let mut slot = error_handle.lock().unwrap();
                    if slot.is_none() { *slot = Some(e); }
                    entries_handle.lock().unwrap().push(std::mem::take(&mut local));
                    ignore::WalkState::Quit
                }
            }
        })
    });

    if let Some(err) = first_error.into_inner().unwrap() {
        return Err(err);
    }

    let mut flat: Vec<FileEntry> = entries
        .into_inner().unwrap()
        .into_iter()
        .flatten()
        .collect();
    flat.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    Ok(FileIndex { entries: flat })
}
```

(The factor-out of `build_walk_builder` and `result_to_entry`
is a refactor of today's monolithic `walk()` body. Both are
private helpers in `walker.rs`. The `result_to_entry` signature
returns `Result<Option<FileEntry>>` so the worker closure can
distinguish "skip" from "error" cleanly.)

**Per-thread accumulation pattern:** the canonical
`build_parallel` example in the `ignore` crate docs uses a
`Builder` factory that returns a fresh closure per worker
thread. Each closure owns a `Vec<FileEntry>` it pushes into
locklessly; on `WalkState::Quit` or completion the closure
appends its `Vec` to the shared `Mutex<Vec<Vec<FileEntry>>>`.
We pay the lock cost once per worker, not once per entry. The
sketch inlines this; the actual implementation should follow
the `ignore` crate's documented pattern verbatim.

**Complexity estimate:** ~1 day. The bulk of the work is the
walker rewrite (~80 lines net new); the unit tests (run twice,
assert equal) and the `bench-compare` validation pass take an
hour each.

## Tests

**Existing tests in `walker.rs`** (already there at v0.8.2):
- `walk_excludes_dot_git_directory`
- `walk_respects_gitignore_when_enabled`
- `walk_includes_gitignored_paths_when_respect_gitignore_false`
- `walk_applies_extra_ignores_as_excludes`
- `walk_invalid_extra_ignore_pattern_surfaces_error`
- `walk_emits_files_with_correct_size`

All assert "contains" not equality, so they continue to pass
unchanged.

**New tests for v0.9.1:**
- `walk_output_is_deterministic_across_runs` — call `walk()`
  twice on the same tree (~50 files spanning 5 directories);
  assert `paths(&idx_a) == paths(&idx_b)`. Most direct guard
  against a forgotten sort.
- `walk_output_is_alphabetically_sorted` — assert the returned
  `Vec<FileEntry>` is sorted by `path`. Catches a sort that
  silently breaks (e.g. `sort_by` argument flipped).
- `walk_propagates_first_error` — synthesise a tree containing
  a path the metadata call cannot stat (e.g. a broken symlink
  with `follow_links(true)` — the existing walker config follows
  links). Assert `walk()` returns `Err`. The exact error message
  can vary across the parallel scheduling, so assert on the
  error variant, not the path field.
- **(Concurrency stress)** `walk_handles_thousand_files` — 1k
  synthetic files across 16 directories; assert exactly 1k file
  entries in the index and the order is the same as a
  `Vec<PathBuf>` we sort manually.

**Bench validation per phase:**
- `walker.rs` micro-bench — currently exercises 100 / 1k / 10k.
  v0.9.1 should improve at 1k and 10k; 100 may regress slightly
  (thread spawn cost dominates at small N — that's fine).
- `rule_engine.rs` micro-bench — total `alint check` time.
  v0.9.1 should improve at 1k and 10k; 100 should stay flat.

The `bench-compare --threshold 10` invocation against the
v0.7.0 baseline runs at v0.9.1 PR time. The 100-file walker
case may need its threshold bumped to allow a small regression
at that size; if so, document it in the v0.9.1 PR description
rather than blanket-disable the gate.

**E2E:** the existing `crates/alint-e2e/` scenarios all pass
through the walker. v0.9.1 should produce byte-identical
outputs across the entire e2e suite. The CI lane is the
existence test; no new scenario fixtures are needed.

## Open questions

1. **Fail-fast vs. collect-all on walker errors.** Today's
   sequential walker fails fast on the first I/O error
   (`?` inside `for result in builder.build()`). Lean fail-fast
   for v0.9.1 — preserves current behaviour, simpler error
   plumbing. If users actually want a "report all bad entries"
   mode, that's a v0.9.x point release with a `--keep-going`
   flag, not a v0.9.1 design choice.
2. **Should `WalkOptions` grow a `threads: usize` field?**
   Lean no for v0.9.1. Defaults are fine; users with thread
   budgets are rare; adding a knob commits us to documenting
   it in the schema. Revisit if a real adopter complains.
3. **Should we fold a `HashMap<PathBuf, &FileEntry>` index
   build into the walker?** `FileIndex::find_file` is a linear
   scan today (`walker.rs:39`). At 100k files it's measurably
   slow for cross-file rules that consult it per match. Lean
   defer to a v0.9.x point release — adding the HashMap is
   independent of the parallel walker change and shouldn't
   block v0.9.1 review.
4. **Should the sort be `sort_by` (stable) instead of
   `sort_unstable_by`?** Stable sort preserves insertion order
   on ties, which matters when paths compare equal. They never
   compare equal in `FileIndex` (every entry has a unique
   relative path from the strip-prefix step). `sort_unstable`
   is correct and ~20% faster; lean unstable.
