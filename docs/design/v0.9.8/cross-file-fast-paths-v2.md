# Cross-file dispatch fast paths, round 2

Status: Design draft, written 2026-05-02 after the v0.9.5 / v0.9.6
/ v0.9.7 macro-bench backfill captured the S7-1M cliff at 652 s.

## Problem

v0.9.5 fixed the cross-file dispatch cliff for `for_each_dir` by
adding a lazy `path_set: OnceLock<HashSet<Arc<Path>>>` to
`FileIndex` and routing `file_exists` literal-path lookups
through it. 1M S3 went from 731.86 s → 11.19 s (65×).

The v0.9.6/v0.9.7 backfill bench surfaced that S7 (cross-file
relational, 6 kinds) was never measured at 1M for v0.9.5 — only
S3 was. v0.9.7's full S1-S9 × 1M capture exposes:

| 1M cell | v0.9.7 | Notes |
|---|---:|---|
| S3 (workspace bundle) | 12 s | v0.9.5 fix territory. Fast. |
| **S7 (six cross-file kinds)** | **~650 s** | The cliff. |

S7 stresses six cross-file kinds: `pair`, `unique_by`,
`for_each_dir`, `for_each_file`, `dir_only_contains`,
`every_matching_has`. Reading their implementations:

- `pair` already uses `contains_file` (O(1)). Fast.
- `unique_by` does a single `for entry in ctx.index.files()` pass
  + HashMap-based dedup. O(N), no quadratic.
- `for_each_dir` / `for_each_file` / `every_matching_has` use
  the shared `evaluate_for_each` helper. Per-dir nested-rule
  build + eval. Fast for `file_exists` literals (post-v0.9.5);
  fall back to glob walk for non-literal nested rule patterns.
- **`dir_only_contains`** at `crates/alint-rules/src/dir_only_contains.rs:75-95`
  has a nested loop: `for dir in dirs() { for file in files() {
  is_direct_child(file, dir) ... } }`. O(D × N). At 1M files ×
  5K dirs = **5 billion ops**.
- **`dir_contains`** at `crates/alint-rules/src/dir_contains.rs:77-99`
  is worse: `for dir in dirs() { for matcher in matchers {
  ctx.index.entries.iter().any(...) } }`. O(D × R × N).

These two are the cliffs. v0.9.8 fixes them with the same
"lazy-OnceLock-on-FileIndex" pattern v0.9.5 established.

## Surface area

Three new accessors on `FileIndex`, all backed by `OnceLock`:

```rust
// crates/alint-core/src/walker.rs
pub struct FileIndex {
    pub entries: Vec<FileEntry>,

    // Existing (v0.9.5):
    path_set: OnceLock<HashSet<Arc<Path>>>,

    // New (v0.9.8):
    /// Map from directory path → indices of its DIRECT children
    /// in `entries`. Built once on first call; the inner Vec is
    /// pre-allocated to the right capacity per dir.
    parent_to_children: OnceLock<HashMap<Arc<Path>, Vec<usize>>>,
    /// Map from directory path → file children's basenames as
    /// borrowed `&str` slices into `entries[i].path`. Built once
    /// per (FileIndex, *first call*).
    file_basenames: OnceLock<HashMap<Arc<Path>, Vec<&'static str>>>,
    // ^ The 'static lifetime is a placeholder; in practice this
    //   is `&self`-bound. May need a `'self` lifetime wrapper or
    //   a separate `FileBasenamesView<'a>` newtype to express it.
}

impl FileIndex {
    /// Direct children of `dir` (files + subdirs), as indices
    /// into `entries`. Empty slice if `dir` has no children or
    /// isn't in the index.
    ///
    /// O(N) build (lazy, on first call across any dir);
    /// O(1) per-dir lookup post-build.
    ///
    /// The `usize` indices avoid lifetime juggling with
    /// `&FileEntry` references — callers do
    /// `let entry = &index.entries[i]` at the use site.
    pub fn children_of(&self, dir: &Path) -> &[usize];

    /// Direct file children's basenames under `dir` (excludes
    /// subdirectories). Borrowed from `entries[i].path`.
    ///
    /// Pre-extracts `path.file_name().and_then(|s| s.to_str())`
    /// once per file; saves the per-call extraction in
    /// `dir_contains`'s matcher loop.
    pub fn file_basenames_of(&self, dir: &Path) -> &[&str];

    /// Recursive descendants of `dir` (files + subdirs), as a
    /// lazy iterator over `children_of` walked depth-first.
    ///
    /// Does NOT materialise the full subtree (root descendants
    /// = all files would cost O(N) memory). Yields entries one
    /// at a time so callers can short-circuit.
    pub fn descendants_of<'a>(
        &'a self,
        dir: &'a Path,
    ) -> impl Iterator<Item = &'a FileEntry> + 'a;
}
```

The `'static` placeholder for `file_basenames` will likely need a
helper type or self-referential pattern (probably
`Cow<'self, [Cow<'self, str>]>` in practice; will refine in
Phase B). If it's awkward, fall back to a lookup that returns
`Vec<&str>` per call (built from `children_of`), accepting the
per-call basename extraction cost.

## Semantics

For each cross-file rule that previously scanned `entries.iter()`:

```rust
// Before (dir_only_contains.rs:81)
for file in ctx.index.files() {
    if !is_direct_child(&file.path, &dir.path) {
        continue;
    }
    // ... per-child logic
}

// After
for &i in ctx.index.children_of(&dir.path) {
    let file = &ctx.index.entries[i];
    if file.is_dir { continue; }
    // ... per-child logic
}
```

For `dir_contains`'s two-level scan:

```rust
// Before (dir_contains.rs:84)
let found = ctx.index.entries.iter().any(|e| {
    if e.path.parent() != Some(&dir.path) { return false; }
    e.path
        .file_name()
        .and_then(|s| s.to_str())
        .is_some_and(|basename| matcher.is_match(basename))
});

// After
let found = ctx.index.file_basenames_of(&dir.path)
    .iter()
    .any(|basename| matcher.is_match(basename));
```

Cost model:

- One-time `parent_to_children` build: O(N) where N = `entries.len()`.
  At 1M, this is ~50 ms (HashMap::insert is the dominant cost,
  not Path hashing — Arc<Path>'s Hash impl forwards to OsStr).
- Per-rule `children_of(dir)`: O(1) HashMap lookup + slice return.
- Per-dir-iteration: O(`|children_of(dir)|`) instead of O(N).

For `dir_only_contains` at 1M / 5K-dir / ~200 files-per-dir:
- Before: 5K × 1M = 5B ops + per-file path comparison
- After: 50 ms build + 5K × 200 = 1M ops + per-file string match

Projected speedup: 100× on `dir_only_contains`. Same shape on
`dir_contains` (with an additional per-rule basename matcher
inner loop, but that's `R × children_per_dir` ≈ a few × 200,
still bounded).

## False-positive surface

- **Path equality on `Arc<Path>`.** The HashMap key is `Arc<Path>`;
  HashMap lookups use `Path::eq`/`Path::hash` (via the `Borrow`
  impl). Cross-platform path canonicalisation (Windows
  case-insensitive matching, Unix case-sensitive) is the same
  shape as the existing `path_set`. Cross-platform tests gated
  by the existing Windows CI matrix.
- **Lazy build under `--changed` mode.** `Engine::run` builds a
  `filtered_index` for `--changed` mode. The new accessors live
  on `FileIndex` so a filtered index has its own (smaller) lazy
  cache. No bleeding.
- **Borrow lifetimes for `file_basenames_of`.** Returning
  `&[&str]` requires the inner `&str`s to live at least as long
  as `&self`. The actual `OsStr` lives in `entries[i].path`'s
  Arc, which stays alive for `&self`'s lifetime — so the
  borrow is sound. Likely needs careful expression in Rust;
  Phase B may settle for a returns-`Vec<&str>` API if the
  self-referential cache is too painful.
- **`descendants_of` infinite loop.** Lazy iterator state must
  not get stuck if `children_of(parent) == [parent]` (i.e.,
  symlink loop). The walker already excludes symlinks by
  default; defensive: cap recursion depth at 256 and emit a
  one-line tracing warning if hit.

## Implementation notes

Per-phase landing strategy:

**Phase B (FileIndex) — single commit:**
1. Add the three OnceLock fields + accessor methods to
   `crates/alint-core/src/walker.rs`.
2. Add the debug-only `trace_index_build!` macro per
   [`README.md`](./README.md) "Tracing instrumentation policy".
3. Unit tests in `walker.rs::tests`:
   - Empty index
   - Single dir with no children
   - Multi-level nesting (files at root, files in subdir, files
     in subsubdir)
   - Repeated calls (memoisation)
   - `--changed` filtered index has its own cache
   - `descendants_of` walks subtree correctly
   - `descendants_of(root)` yields every file
4. Micro-bench in `crates/alint-bench/benches/`: new `walker`
   bench cells for `children_of` build cost at 100/1k/10k/100k/1M.

**Phase C (rule refactors) — three commits:**

1. `crates/alint-rules/src/dir_only_contains.rs` — swap inner
   loop. Existing tests + e2e scenarios stay green.
2. `crates/alint-rules/src/dir_contains.rs` — swap inner loop +
   use `file_basenames_of` for matcher.
3. `crates/alint-rules/src/{for_each_dir,for_each_file,every_matching_has}.rs`
   — audit `evaluate_for_each` + nested-rule fast paths. May add
   a `Scope::matches_in_dir(dir, basenames)` helper that uses
   `file_basenames_of` when the nested rule's `paths:` is a
   simple basename glob.

**Phase D (tests + audit) — single commit:**

- `crates/alint-e2e/tests/coverage_audit_cross_file_dispatch.rs`
  — asserts each cross-file rule's `evaluate` calls
  `children_of` (not `entries.iter()`) by a static-source-grep
  audit. Emits a soft failure (`eprintln!`) listing rules still
  on the slow path so the next contributor knows what to fix.
- `crates/alint-e2e/scenarios/check/cross_file/v0.9.8-fast-paths/`
  — 4-5 new YAMLs against deep-tree fixtures (10K files in 200
  dirs, etc.) that would have hit the cliff at v0.9.7.
- `proptest`: `dir_only_contains_old_vs_new` — generate a
  random tree + dir_only_contains config; assert old shape
  produces identical violations to new shape on 1k-file inputs.

Complexity estimate per phase:

| Phase | Lines touched | Time |
|---|---|---|
| B (FileIndex) | ~150 walker.rs + ~80 tests + ~40 bench | 3 hr |
| C.1 (dir_only_contains) | ~20 swap | 30 min |
| C.2 (dir_contains) | ~30 swap + basenames consumer | 30 min |
| C.3 (for_each_*, every_matching_has audit) | ~80 (evaluate_for_each helper + 3 rule edits) | 4 hr |
| D (tests + audit) | ~300 across 5 files | 2 hr |
| **Total** | **~700** | **~10 hr** |

Plus Phase A (30 min profile) + Phase E (3 hr bench) + Phase F
(30 min release).

## Tests

- Unit (walker.rs::tests): see Phase B list above.
- Integration (alint-rules/tests): re-use existing
  `dir_only_contains` / `dir_contains` integration tests; expect
  them to pass unchanged.
- e2e (alint-e2e/scenarios/check/cross_file/): 4-5 new YAMLs
  in `v0.9.8-fast-paths/` covering the dispatch shapes the
  fast-paths target.
- proptest: A/B old-vs-new on 1k-file inputs (1000 cases).
- Audit: `coverage_audit_cross_file_dispatch.rs` static grep.
- Bench-compare: against v0.9.7 published numbers (just landed
  via the bench-backfill PR). Threshold:
  - 1M S7 must drop > 6.5×.
  - All other 1M cells must stay within ±5 % (no regression on
    the per-file dispatch fast paths).

## Open questions

1. **`file_basenames_of` lifetime.** Self-referential
   `&'self [&'self str]` is awkward in Rust. Three options:
   (a) `OwningRef`-style wrapper, (b) interned-string cache
   (`OnceLock<Vec<Arc<str>>>`), (c) compute per-call and live
   with the cost. Decide in Phase B based on what the type
   signature can express cleanly. Default to (c) if (a) and
   (b) bloat the API.
2. **Should `children_of` distinguish files from subdirs?** Two
   accessors (`file_children_of` + `dir_children_of`) would let
   `dir_only_contains` skip the `if file.is_dir { continue; }`
   filter. Saves 1 branch per child — measurable at 1M? Unclear.
   Add only if Phase E shows it matters.
3. **`descendants_of` ordering — depth-first or level-order?**
   Depth-first matches the existing `entries` order from the
   walker. Level-order would let consumers prune at depth limits
   cheaply. Not used by any current rule; default depth-first.
4. **Cache invalidation on `--changed` filtered index.** A new
   `FileIndex` is built per `Engine::run` call when `--changed`
   is active. The OnceLock is per-instance, so no cross-instance
   bleeding. Verify this with a unit test that runs two
   different filtered indices in sequence.
5. **Bench-compare threshold for 1M S7.** "Below 100 s" is the
   acceptance gate. What if v0.9.8 lands at 80 s but v0.10 needs
   30 s for LSP responsiveness? Then v0.10 picks up where
   v0.9.8 leaves off; not a v0.9.8 problem. Document the gate
   as "below 100 s, target 50 s" so reviewers know what's
   gold-plated vs required.
