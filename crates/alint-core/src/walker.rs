use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use ignore::{
    ParallelVisitor, ParallelVisitorBuilder, WalkBuilder, WalkState, overrides::OverrideBuilder,
};

use crate::error::{Error, Result};

/// Shared, thread-safe sink for the walk's escaping-symlink side-list (M4):
/// `filter_entry` (parallel) pushes each pruned escaping symlink's relative
/// path; `walk` drains it into the [`FileIndex`].
type EscapingSink = Arc<Mutex<Vec<Arc<Path>>>>;

/// Hard cap on a single whole-file read by the per-file engine/rule loops
/// and the direct-read structured-query kinds. Generous — every realistic
/// source / config / manifest is orders of magnitude smaller — yet bounded so
/// a hostile or accidental multi-GB file in a linted repo is skipped instead
/// of OOM-ing the run. Shared source of truth: `alint-rules`'s `read_capped`
/// family re-exports this constant (M3).
///
/// KNOWN LIMITATION (bytes, not tree): this bounds INPUT size, not the parsed
/// `serde_json::Value` tree a structured format expands into, which measures
/// ~10-16x the input (XML worst). A near-cap 256 MiB structured file can thus
/// still peak at multiple GB of RSS, multiplied by the `par_iter` fan-out that
/// parses several files at once — an OOM risk on a memory-limited runner that a
/// byte cap alone does not close. A structured-parse-specific cap and/or a
/// concurrent-parse-memory bound are a tracked follow-up (see
/// `docs/design/format-coverage.md`, "Known limitations").
pub const MAX_ANALYZE_BYTES: u64 = 256 * 1024 * 1024;

/// Read a walked file's bytes, first fast-rejecting (loudly) any file whose
/// walk-time [`FileEntry::size`] exceeds [`MAX_ANALYZE_BYTES`] (no extra `stat`),
/// then bounding the actual read to the cap (`read_bounded`, TOCTOU-safe). The
/// per-file loops read the whole file into memory, so an uncapped read of a
/// committed multi-GB blob would OOM the process; a bounded skip keeps the run
/// alive and observable (M3). A genuine read error (permission, I/O) is logged
/// at `warn` so it's observable with `-v` / `RUST_LOG` rather than silent (L7);
/// a `NotFound` is a silent skip (the benign deleted-between-walk-and-read race).
pub fn read_capped_or_skip(path: &Path, size: u64) -> Option<Vec<u8>> {
    if size > MAX_ANALYZE_BYTES {
        tracing::warn!(
            path = %path.display(),
            size,
            cap = MAX_ANALYZE_BYTES,
            "skipping file larger than the analysis cap"
        );
        return None;
    }
    // M3-F2 (TOCTOU): the walk-time `size` above is a fast reject only — it can
    // be stale, so a file that GREW past the cap between the walk and here would
    // otherwise slip through to an unbounded read. Bound the actual read by
    // BYTES (not the trusted size) so concurrent growth can't force an OOM.
    // `size` is ALSO forwarded as the read buffer's preallocation hint — alint
    // already stat-ed it during the walk, so it costs nothing and lets the read
    // finish in one syscall (see `read_bounded`).
    read_bounded(path, MAX_ANALYZE_BYTES, size)
}

/// Read a whole file bounded to `cap` bytes — TOCTOU-safe: the read itself
/// stops at `cap + 1` bytes, so the file's size *at read time* (not a possibly
/// stale earlier stat / walk-time size) decides. A file that measures `> cap`
/// is skipped (loud `warn`); a missing file is `None`.
///
/// `size_hint` (the walk-time [`FileEntry::size`], already stat-ed by the walk
/// so free here) preallocates the buffer. It is strictly ADVISORY — capacity
/// only, clamped to `cap + 1`, and it NEVER affects what is accepted: the
/// `take(cap + 1)` below is the sole correctness bound, so a stale or hostile
/// hint cannot force an over-read. The preallocation matters because a
/// `Take<File>` (unlike a bare `File`) has no `read_to_end` fstat-preallocation
/// specialization: given an empty `Vec` it grows-and-rereads, issuing several
/// `read()` syscalls per file. That overhead is ~invisible to the deterministic
/// Valgrind-Ir gate (a `read()` is a handful of guest instructions but a real
/// kernel round-trip) yet measurably regressed read-heavy scenarios (S2) on the
/// wall clock. Sizing the buffer up front restores the single-read behaviour
/// `std::fs::read` had before the OOM cap. See
/// docs/benchmarks/investigations/2026-07-v0.14-s2-harness-artifact/.
pub(crate) fn read_bounded(path: &Path, cap: u64, size_hint: u64) -> Option<Vec<u8>> {
    use std::io::Read as _;
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "skipping unreadable file");
            return None;
        }
    };
    let prealloc = usize::try_from(size_hint.min(cap.saturating_add(1))).unwrap_or(0);
    let mut buf = Vec::with_capacity(prealloc);
    match file.take(cap.saturating_add(1)).read_to_end(&mut buf) {
        Ok(_) if u64::try_from(buf.len()).is_ok_and(|n| n > cap) => {
            tracing::warn!(
                path = %path.display(),
                cap,
                "skipping file larger than the analysis cap (grew past its walk-time size)"
            );
            None
        }
        Ok(_) => Some(buf),
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "skipping unreadable file");
            None
        }
    }
}

/// Debug-only tracing for `FileIndex` lazy index builds. Emits a
/// `phase=index_build kind=<name> elapsed_us=N entries=M` event so
/// `xtask bench-scale` profile runs and contributor debugging can
/// see how long the lazy `OnceLock` builds cost. Compiled out
/// entirely in release builds — `Instant::now()` and the event
/// emission are both gated behind `cfg(debug_assertions)`, so
/// users running release binaries pay zero runtime cost for the
/// instrumentation.
#[cfg(debug_assertions)]
macro_rules! trace_index_build {
    ($kind:expr, $start:expr, $entries:expr) => {{
        #[allow(clippy::cast_possible_truncation)]
        let elapsed_us: u64 = $start.elapsed().as_micros() as u64;
        tracing::debug!(
            phase = "index_build",
            kind = $kind,
            elapsed_us = elapsed_us,
            entries = $entries as u64,
            "engine.index",
        );
    }};
}
#[cfg(not(debug_assertions))]
macro_rules! trace_index_build {
    ($kind:expr, $start:expr, $entries:expr) => {};
}

/// A single filesystem entry discovered by the walker.
///
/// `path` is held as [`Arc<Path>`] so per-violation copies are
/// atomic refcount bumps rather than path-byte allocations.
/// Every [`Violation`](crate::rule::Violation) referencing this
/// file shares the same allocation; at 100k violations that's
/// 100k saved `PathBuf` clones.
#[derive(Debug, Clone)]
pub struct FileEntry {
    /// Path relative to the repository root.
    pub path: Arc<Path>,
    pub is_dir: bool,
    pub size: u64,
}

/// The indexed result of one filesystem walk. All rules share this index —
/// the walk happens once per `alint check` invocation.
///
/// `path_set` is a lazy `HashSet<Arc<Path>>` over file entries.
/// Built once on first call to [`FileIndex::contains_file`] /
/// [`FileIndex::file_path_set`] and re-used across all subsequent
/// lookups. Cross-file rules that ask "does this exact path
/// exist?" (most importantly `file_exists` instantiated by
/// `for_each_dir`) hit the set instead of doing an O(N) linear
/// scan over every entry. At 1M files in a 5,000-package
/// monorepo, this turns the fan-out shape from O(D × N) =
/// 5 × 10⁹ ops to O(D) = 5,000 lookups.
///
/// `parent_to_children` (v0.9.8) is a second lazy index — for
/// each directory, the indices of its DIRECT children in
/// `entries`. Cross-file rules that previously scanned all
/// entries per matched dir (`dir_only_contains`, `dir_contains`)
/// now lookup `children_of(dir)` (O(1)) instead of doing a
/// per-dir O(N) scan. Closes the v0.9.5 → v0.9.8 cliff: at 1M
/// files / 5K dirs, `dir_only_contains` drops from 5 billion
/// path-parent comparisons to ~1 million.
#[derive(Debug, Default)]
pub struct FileIndex {
    pub entries: Vec<FileEntry>,
    /// Symlinks whose target ESCAPES the repo root. Pruned from `entries` by the
    /// walk (so no content rule can `root.join()` through them to read out-of-
    /// tree bytes) but recorded here so `no_symlinks` can still flag them (M4).
    /// Relative paths, sorted + deduped. Empty for a fixture-built index.
    escaping_symlinks: Vec<Arc<Path>>,
    path_set: OnceLock<HashSet<Arc<Path>>>,
    parent_to_children: OnceLock<HashMap<Arc<Path>, Vec<usize>>>,
    /// Per-`since`-ref diff sets backing `scope_filter.changed_since:`.
    /// Populated once by the engine before per-file dispatch (a ref
    /// that resolves to "not a git repo" stores an empty set, so the
    /// predicate silently matches nothing). Read lock-free in the
    /// hot loop after `set`.
    changed_paths: OnceLock<HashMap<String, HashSet<std::path::PathBuf>>>,
    /// Per-predicate manifest-derived path sets backing
    /// `scope_filter.include_manifest_paths:` / `exclude_manifest_paths:`. Keyed
    /// by each predicate's canonical `(source, extract, derive_target)` cache
    /// key, so rules sharing a config share one resolved set. Populated once by
    /// the engine before per-file dispatch (a missing key = empty set, so the
    /// predicate contributes nothing). Read lock-free in the hot loop after
    /// `set`.
    manifest_paths: OnceLock<HashMap<String, crate::scope_filter::ManifestSet>>,
    /// Evaluated `facts:` values, cached so repeated [`Engine::run_for_file`]
    /// calls (the LSP per-keystroke path) don't re-scan the tree.
    /// ASSUMPTION: a given index is evaluated by one engine's fact set
    /// for its lifetime (true in alint — an index is walked fresh per
    /// run / per LSP session). Read lock-free after `set`.
    facts: OnceLock<crate::facts::FactValues>,
}

impl FileIndex {
    /// Construct a [`FileIndex`] from raw entries. Equivalent to
    /// `FileIndex { entries, ..Default::default() }` but spelled
    /// out so test/bench fixtures don't have to know about the
    /// internal lazy `path_set` field.
    pub fn from_entries(entries: Vec<FileEntry>) -> Self {
        Self::from_entries_with_escaping(entries, Vec::new())
    }

    /// Like [`from_entries`](Self::from_entries) but also records the walk's
    /// escaping-symlink side-list (M4). Used by [`walk`]; fixtures use the plain
    /// constructor (empty side-list).
    pub(crate) fn from_entries_with_escaping(
        entries: Vec<FileEntry>,
        escaping_symlinks: Vec<Arc<Path>>,
    ) -> Self {
        Self {
            entries,
            escaping_symlinks,
            path_set: OnceLock::new(),
            parent_to_children: OnceLock::new(),
            changed_paths: OnceLock::new(),
            manifest_paths: OnceLock::new(),
            facts: OnceLock::new(),
        }
    }

    /// Symlinks whose target escapes the repo root — pruned from `entries` (so no
    /// content rule reads through them) but surfaced here for `no_symlinks` (M4).
    /// Relative paths.
    #[must_use]
    pub fn escaping_symlinks(&self) -> &[Arc<Path>] {
        &self.escaping_symlinks
    }

    /// The cached evaluated `facts:` values, if [`set_facts`](Self::set_facts)
    /// has run. `None` before the first evaluation.
    #[must_use]
    pub fn cached_facts(&self) -> Option<&crate::facts::FactValues> {
        self.facts.get()
    }

    /// Cache the evaluated facts (no-op if already set). The engine
    /// populates this on the first `run_for_file` so subsequent
    /// per-file re-evaluations reuse it instead of re-scanning the tree.
    pub fn set_facts(&self, values: crate::facts::FactValues) {
        let _ = self.facts.set(values);
    }

    /// Look up the cached changed-paths set for a `since` ref. `None`
    /// means the cache wasn't populated for this ref (the engine only
    /// resolves refs that appear on a per-file rule's
    /// `scope_filter.changed_since:`); the predicate treats that as
    /// "no file matches".
    #[must_use]
    pub fn changed_paths(&self, since: &str) -> Option<&HashSet<std::path::PathBuf>> {
        self.changed_paths.get()?.get(since)
    }

    /// `true` once the engine has populated the changed-paths cache.
    #[must_use]
    pub fn changed_paths_initialized(&self) -> bool {
        self.changed_paths.get().is_some()
    }

    /// Populate the changed-paths cache (engine-only, once per run,
    /// before parallel dispatch). A no-op if already set, so re-using
    /// one index across `run` + `fix` is safe.
    pub fn set_changed_paths(&self, map: HashMap<String, HashSet<std::path::PathBuf>>) {
        let _ = self.changed_paths.set(map);
    }

    /// The whole resolved `changed_since` diff map, if populated. Like
    /// [`Self::manifest_paths_map`], the engine resolves diffs against the FULL
    /// index and copies the result onto the pre-filtered `--changed` index (whose
    /// own entries omit the ref-diff set), so a per-file `scope_filter.changed_since:`
    /// still resolves under `--changed` instead of silently matching nothing.
    #[must_use]
    pub(crate) fn changed_paths_map(
        &self,
    ) -> Option<&HashMap<String, HashSet<std::path::PathBuf>>> {
        self.changed_paths.get()
    }

    /// Look up the cached manifest-derived path set for a predicate's cache key.
    /// `None` means the cache wasn't populated for this key (manifest absent /
    /// unresolved); the predicate treats it as the empty set.
    #[must_use]
    pub(crate) fn manifest_paths(
        &self,
        cache_key: &str,
    ) -> Option<&crate::scope_filter::ManifestSet> {
        self.manifest_paths.get()?.get(cache_key)
    }

    /// `true` once the engine has populated the manifest-paths cache.
    #[must_use]
    pub fn manifest_paths_initialized(&self) -> bool {
        self.manifest_paths.get().is_some()
    }

    /// Populate the manifest-paths cache (engine-only, once per run, before
    /// parallel dispatch). A no-op if already set, so re-using one index across
    /// `run` + `fix` is safe.
    pub fn set_manifest_paths(&self, map: HashMap<String, crate::scope_filter::ManifestSet>) {
        let _ = self.manifest_paths.set(map);
    }

    /// The whole resolved manifest-paths map, if populated. The engine resolves
    /// manifests against the FULL index (so `find_file` reaches them) and copies
    /// the result onto the pre-filtered `--changed` index (whose own entries omit
    /// unchanged manifests), so per-file `matches` against the filtered index
    /// still sees the set. The declared path set is independent of which files
    /// the run visits, so the same map is valid for both.
    #[must_use]
    pub(crate) fn manifest_paths_map(
        &self,
    ) -> Option<&HashMap<String, crate::scope_filter::ManifestSet>> {
        self.manifest_paths.get()
    }

    pub fn files(&self) -> impl Iterator<Item = &FileEntry> {
        self.entries.iter().filter(|e| !e.is_dir)
    }

    pub fn dirs(&self) -> impl Iterator<Item = &FileEntry> {
        self.entries.iter().filter(|e| e.is_dir)
    }

    pub fn total_size(&self) -> u64 {
        self.files().map(|f| f.size).sum()
    }

    /// Get (lazily building on first call) the hash-indexed set
    /// of all *file* (non-dir) paths in this index. Subsequent
    /// calls return the cached set. Concurrent first calls are
    /// safe (`OnceLock` ensures a single initialiser wins).
    pub fn file_path_set(&self) -> &HashSet<Arc<Path>> {
        self.path_set.get_or_init(|| {
            #[cfg(debug_assertions)]
            let start = std::time::Instant::now();
            let set: HashSet<Arc<Path>> = self
                .entries
                .iter()
                .filter(|e| !e.is_dir)
                .map(|e| Arc::clone(&e.path))
                .collect();
            trace_index_build!("path_set", start, self.entries.len());
            set
        })
    }

    /// O(1) "does this exact relative path exist as a file?"
    /// query. Triggers the lazy build of the path set on first
    /// call. Use this instead of iterating `files()` whenever a
    /// rule needs to check a fully-qualified path — at scale,
    /// the hash lookup is several orders of magnitude faster.
    pub fn contains_file(&self, rel: &Path) -> bool {
        self.file_path_set().contains(rel)
    }

    /// Find a file entry by its exact relative path. Uses the
    /// lazy path set for the existence check, then re-scans
    /// entries linearly to return the matching `&FileEntry`
    /// (entries are pinned, but the set stores `Arc<Path>` keys
    /// not direct entry references). Most callers want the
    /// boolean answer — prefer [`FileIndex::contains_file`].
    pub fn find_file(&self, rel: &Path) -> Option<&FileEntry> {
        if !self.contains_file(rel) {
            return None;
        }
        self.files().find(|e| &*e.path == rel)
    }

    // ── v0.9.8 — parent_to_children index ────────────────────────

    /// Direct children of `dir`, as indices into [`Self::entries`].
    /// Triggers the lazy build of the parent → children map on
    /// first call across any directory.
    ///
    /// Returns an empty slice when `dir` has no children or isn't
    /// in the index. Indices are stable across the lifetime of
    /// `&self` — use them via `&self.entries[i]` at the call site
    /// to dereference.
    ///
    /// Build cost: O(N) (one pass over `entries`, one `HashMap`
    /// insert per entry). Lookup cost: O(1) `HashMap` probe.
    /// Replaces the O(D × N) `for dir in dirs() { for file in
    /// files() { is_direct_child(file, dir) ... } }` shape that
    /// `dir_only_contains` and `dir_contains` previously used.
    /// At 1M files × 5K matched dirs, that's a 5,000× reduction
    /// in total comparison count.
    pub fn children_of(&self, dir: &Path) -> &[usize] {
        let map = self.parent_to_children.get_or_init(|| {
            #[cfg(debug_assertions)]
            let start = std::time::Instant::now();
            let mut map: HashMap<Arc<Path>, Vec<usize>> = HashMap::new();
            for (idx, entry) in self.entries.iter().enumerate() {
                let Some(parent) = entry.path.parent() else {
                    continue;
                };
                // Look up an existing key by &Path borrow first to
                // avoid the per-entry Arc clone in the common case
                // (most parents already have a child indexed).
                if let Some(slot) = map.get_mut(parent) {
                    slot.push(idx);
                    continue;
                }
                // First child for this parent — promote the
                // borrowed &Path to an Arc<Path>. Prefer cloning
                // the Arc from a sibling entry whose path IS the
                // parent dir (so the HashMap key + the entries[i]
                // Arc point at the same allocation), but fall back
                // to allocating a fresh Arc if the parent dir
                // isn't itself in the index (root-level files,
                // ancestor dirs the walker excluded, etc.).
                let key: Arc<Path> = self
                    .entries
                    .iter()
                    .find(|e| e.is_dir && &*e.path == parent)
                    .map_or_else(|| Arc::<Path>::from(parent), |e| Arc::clone(&e.path));
                map.insert(key, vec![idx]);
            }
            trace_index_build!("parent_to_children", start, self.entries.len());
            map
        });
        map.get(dir).map_or(&[], Vec::as_slice)
    }

    /// Direct file children's basenames under `dir`. Filters out
    /// subdirectories — pure file basenames only. Returns an
    /// iterator borrowing into `entries[i].path` for each match;
    /// no allocation per call (the underlying `Path::file_name()`
    /// returns a borrow into the `Arc<Path>`).
    ///
    /// Built on top of [`Self::children_of`]. Cross-file rules
    /// like `dir_contains` whose hot path is "does this dir have
    /// any file matching this basename matcher?" use this to skip
    /// the per-call `path.file_name().and_then(|s| s.to_str())`
    /// extraction and the `entries.iter().any(...)` scan in one
    /// shot.
    ///
    /// Files whose basename isn't valid UTF-8 are silently
    /// dropped from the iterator — same shape as the existing
    /// path-string consumers.
    pub fn file_basenames_of<'a>(&'a self, dir: &Path) -> impl Iterator<Item = &'a str> + 'a {
        self.children_of(dir).iter().filter_map(move |&i| {
            let e = &self.entries[i];
            if e.is_dir {
                return None;
            }
            e.path.file_name().and_then(|s| s.to_str())
        })
    }

    /// All descendants under `dir` (files + subdirs), recursive,
    /// depth-first. Built on top of [`Self::children_of`]; does
    /// NOT materialise the full subtree as a Vec (root descendants
    /// = every entry would cost O(N) memory, defeating the lazy
    /// design). Yields entries one at a time so callers can
    /// short-circuit cleanly via `take_while` / `find` / etc.
    ///
    /// Cycle defense: a stack-based walk with no per-iteration
    /// cycle check. The walker (`crate::walk`) calls
    /// `WalkBuilder::follow_links(true)` to traverse through
    /// symlinks, and the underlying `ignore` crate carries
    /// cycle detection — an ancestor-self symlink emits an error
    /// and the walker continues without recursing. The entries
    /// vec is therefore acyclic by construction; adding a per-
    /// step cycle check would cost ~10 ns per yielded entry for
    /// a guarantee that's already established at walker time.
    pub fn descendants_of<'a>(&'a self, dir: &'a Path) -> impl Iterator<Item = &'a FileEntry> + 'a {
        DescendantsIter {
            index: self,
            stack: vec![self.children_of(dir).iter().copied().rev().collect()],
        }
    }
}

/// Stack-of-iterators state for [`FileIndex::descendants_of`]. Each
/// element of the outer stack is the remaining children of one
/// ancestor dir to visit, in reverse order so `pop()` yields them
/// in the original (sorted) order. When a yielded entry is itself
/// a directory, its children are pushed as a fresh frame for the
/// next iteration to descend into.
struct DescendantsIter<'a> {
    index: &'a FileIndex,
    stack: Vec<Vec<usize>>,
}

impl<'a> Iterator for DescendantsIter<'a> {
    type Item = &'a FileEntry;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let frame = self.stack.last_mut()?;
            let Some(idx) = frame.pop() else {
                self.stack.pop();
                continue;
            };
            let entry = &self.index.entries[idx];
            if entry.is_dir {
                let children = self.index.children_of(&entry.path);
                if !children.is_empty() {
                    self.stack.push(children.iter().copied().rev().collect());
                }
            }
            return Some(entry);
        }
    }
}

#[derive(Debug, Clone)]
pub struct WalkOptions {
    pub respect_gitignore: bool,
    pub extra_ignores: Vec<String>,
}

impl Default for WalkOptions {
    fn default() -> Self {
        Self {
            respect_gitignore: true,
            extra_ignores: Vec::new(),
        }
    }
}

pub fn walk(root: &Path, opts: &WalkOptions) -> Result<FileIndex> {
    let (builder, escaping) = build_walk_builder(root, opts)?;

    // Per-thread accumulators land in `out_entries`; the first
    // error wins and stops the walk via `WalkState::Quit` (the
    // worker that sees it sets the slot, others poll it on each
    // visit and bail). Single-writer semantics keep the lock
    // cost low — it's held once per worker on push, not per
    // entry.
    let out_entries: Arc<Mutex<Vec<Vec<FileEntry>>>> = Arc::new(Mutex::new(Vec::new()));
    let error_slot: Arc<Mutex<Option<Error>>> = Arc::new(Mutex::new(None));
    let root_owned: Arc<PathBuf> = Arc::new(root.to_path_buf());

    let mut visitor_builder = WalkVisitorBuilder {
        root: Arc::clone(&root_owned),
        error_slot: Arc::clone(&error_slot),
        out_entries: Arc::clone(&out_entries),
    };
    builder.build_parallel().visit(&mut visitor_builder);

    if let Some(err) = error_slot.lock().expect("walker error slot lock").take() {
        return Err(err);
    }

    // Flatten the per-thread `Vec`s into one `Vec`. We deliberately
    // do NOT preserve insertion order across threads — the
    // sort_unstable_by below restores a deterministic ordering by
    // relative path, which is the contract callers (snapshot tests,
    // formatters) actually depend on.
    let mut entries: Vec<FileEntry> = out_entries
        .lock()
        .expect("walker out-entries lock")
        .drain(..)
        .flatten()
        .collect();
    entries.sort_unstable_by(|a, b| a.path.cmp(&b.path));

    // M4: the escaping/broken symlinks the filter pruned. Sort + dedup for a
    // deterministic side-list (the walk is parallel, order is non-deterministic).
    let mut escaping_symlinks: Vec<Arc<Path>> = escaping
        .lock()
        .expect("walker escaping-symlink lock")
        .drain(..)
        .collect();
    escaping_symlinks.sort_unstable();
    escaping_symlinks.dedup();
    Ok(FileIndex::from_entries_with_escaping(
        entries,
        escaping_symlinks,
    ))
}

/// Build the `ignore::WalkBuilder` we run today. Pure factor-out
/// of the original `walk()` body's setup half so both the
/// sequential test path and the parallel runtime path stay in
/// sync.
fn build_walk_builder(root: &Path, opts: &WalkOptions) -> Result<(WalkBuilder, EscapingSink)> {
    let mut builder = WalkBuilder::new(root);
    builder
        .standard_filters(opts.respect_gitignore)
        .hidden(false)
        .follow_links(true)
        .require_git(false);

    // Always exclude `.git/` — descending into git's internal
    // packfiles + loose objects is wasted work for every alint
    // rule (none of them target `.git/objects/*`), and it races
    // git's auto-gc / pack-rewrite on large repos. We set
    // `hidden(false)` and `require_git(false)` so the `ignore`
    // crate doesn't apply its own implicit `.git/` exclusion;
    // this override puts it back.
    let mut overrides_builder = OverrideBuilder::new(root);
    overrides_builder
        .add("!.git")
        .map_err(|e| Error::Other(format!("ignore pattern .git: {e}")))?;
    for pattern in &opts.extra_ignores {
        let pattern = if pattern.starts_with('!') {
            pattern.clone()
        } else {
            format!("!{pattern}")
        };
        overrides_builder
            .add(&pattern)
            .map_err(|e| Error::Other(format!("ignore pattern {pattern:?}: {e}")))?;
    }
    let overrides = overrides_builder
        .build()
        .map_err(|e| Error::Other(format!("failed to build overrides: {e}")))?;
    builder.overrides(overrides);

    // Prune symlinks whose target escapes the repo root. With
    // `follow_links(true)`, a followed out-of-tree symlink would
    // otherwise be indexed under its in-tree path — and an out-of-tree
    // symlink-to-dir would have its children descended and indexed too
    // — letting content rules read outside the tree (the untrusted-PR-
    // content half of the path-confinement threat; see
    // `docs/design/v0.12/path-confinement.md`). In-tree symlinks are
    // still followed. Canonicalising both sides also handles a root
    // that itself lives under a symlink (e.g. macOS `/tmp` →
    // `/private/tmp`). Only symlink entries pay the `canonicalize`
    // syscall; the common non-symlink path is a bare `true`, so the
    // walk's cost is unchanged for trees without symlinks.
    let canonical_root = root.canonicalize().ok();
    // M4 side-list: escaping/broken symlinks are pruned from `entries` (below,
    // via the `false` return) so no content rule reads through them, but their
    // relative paths are recorded here so `no_symlinks` can still flag them.
    let escaping: Arc<Mutex<Vec<Arc<Path>>>> = Arc::new(Mutex::new(Vec::new()));
    let escaping_filter = Arc::clone(&escaping);
    let root_for_rel = root.to_path_buf();
    builder.filter_entry(move |entry| {
        if !entry.path_is_symlink() {
            return true;
        }
        let keep = match (&canonical_root, entry.path().canonicalize()) {
            (Some(root), Ok(target)) => target.starts_with(root),
            // Broken / unresolvable symlink (or an un-canonicalisable
            // root) — can't be safely read anyway; prune it.
            _ => false,
        };
        if !keep {
            if let Ok(rel) = entry.path().strip_prefix(&root_for_rel) {
                if !rel.as_os_str().is_empty() {
                    if let Ok(mut v) = escaping_filter.lock() {
                        v.push(Arc::from(rel));
                    }
                }
            }
        }
        keep
    });
    Ok((builder, escaping))
}

/// Convert one `ignore::DirEntry` (or its error) into a
/// `FileEntry`. Returns `Ok(None)` for entries we deliberately
/// skip (the walk root itself, or anything outside the root).
/// The error path produces the same `Error::Io` / `Error::Walk`
/// variants the sequential walker did, so callers see no
/// behavioural change.
fn result_to_entry(
    root: &Path,
    result: std::result::Result<ignore::DirEntry, ignore::Error>,
) -> Result<Option<FileEntry>> {
    let entry = result?;
    let abs = entry.path();
    let Ok(rel) = abs.strip_prefix(root) else {
        return Ok(None);
    };
    if rel.as_os_str().is_empty() {
        return Ok(None);
    }
    let metadata = entry.metadata().map_err(|e| Error::Io {
        path: abs.to_path_buf(),
        source: std::io::Error::other(e.to_string()),
    })?;
    // Index only regular files and directories. A special file (FIFO/named
    // pipe, socket, char/block device) is not lintable content, and — worse —
    // opening a FIFO `O_RDONLY` BLOCKS until a writer appears, so a per-file
    // content rule reading one would hang the whole run forever. `follow_links`
    // is on, so a symlink to a special file resolves here to its (non-regular)
    // target and is dropped too. Skip it rather than index it as a size-0 file.
    if !metadata.is_dir() && !metadata.is_file() {
        return Ok(None);
    }
    Ok(Some(FileEntry {
        path: Arc::from(rel),
        is_dir: metadata.is_dir(),
        size: if metadata.is_file() {
            metadata.len()
        } else {
            0
        },
    }))
}

/// Per-thread visitor: accumulates `FileEntry`s in a thread-
/// local `Vec`. On `Drop` (one per worker thread, when the
/// walk finishes), it appends the local `Vec` to the shared
/// out-entries slot. The lock is held once per worker, not
/// per entry — keeping it off the hot path.
struct WalkVisitor {
    root: Arc<PathBuf>,
    entries: Vec<FileEntry>,
    error_slot: Arc<Mutex<Option<Error>>>,
    out_entries: Arc<Mutex<Vec<Vec<FileEntry>>>>,
}

impl ParallelVisitor for WalkVisitor {
    fn visit(&mut self, result: std::result::Result<ignore::DirEntry, ignore::Error>) -> WalkState {
        // Cheap exit when another worker has already failed:
        // poll the shared slot once per visit. The lock is
        // uncontended in the common (no-error) case.
        if self
            .error_slot
            .lock()
            .expect("walker error slot lock")
            .is_some()
        {
            return WalkState::Quit;
        }
        match result_to_entry(&self.root, result) {
            Ok(Some(entry)) => {
                self.entries.push(entry);
                WalkState::Continue
            }
            Ok(None) => WalkState::Continue,
            Err(err) => {
                let mut slot = self.error_slot.lock().expect("walker error slot lock");
                if slot.is_none() {
                    *slot = Some(err);
                }
                WalkState::Quit
            }
        }
    }
}

impl Drop for WalkVisitor {
    fn drop(&mut self) {
        let local = std::mem::take(&mut self.entries);
        if local.is_empty() {
            return;
        }
        if let Ok(mut out) = self.out_entries.lock() {
            out.push(local);
        }
    }
}

struct WalkVisitorBuilder {
    root: Arc<PathBuf>,
    error_slot: Arc<Mutex<Option<Error>>>,
    out_entries: Arc<Mutex<Vec<Vec<FileEntry>>>>,
}

impl<'s> ParallelVisitorBuilder<'s> for WalkVisitorBuilder {
    fn build(&mut self) -> Box<dyn ParallelVisitor + 's> {
        Box::new(WalkVisitor {
            root: Arc::clone(&self.root),
            entries: Vec::new(),
            error_slot: Arc::clone(&self.error_slot),
            out_entries: Arc::clone(&self.out_entries),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn td() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix("alint-walker-test-")
            .tempdir()
            .unwrap()
    }

    fn touch(root: &Path, rel: &str, content: &[u8]) {
        let abs = root.join(rel);
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(abs, content).unwrap();
    }

    fn paths(idx: &FileIndex) -> Vec<String> {
        // Normalise to forward slashes so assertions can compare
        // against literal `"src/foo.rs"` regardless of host OS.
        // Windows' Path::display() emits `src\foo.rs`.
        idx.entries
            .iter()
            .map(|e| e.path.display().to_string().replace('\\', "/"))
            .collect()
    }

    #[test]
    fn fileindex_files_filters_directories_out() {
        let idx = FileIndex::from_entries(vec![
            FileEntry {
                path: Path::new("a").into(),
                is_dir: true,
                size: 0,
            },
            FileEntry {
                path: Path::new("a/x.rs").into(),
                is_dir: false,
                size: 5,
            },
        ]);
        let files: Vec<_> = idx.files().collect();
        assert_eq!(files.len(), 1);
        assert_eq!(&*files[0].path, Path::new("a/x.rs"));
    }

    #[cfg(unix)]
    #[test]
    fn walk_prunes_symlinks_that_escape_the_root() {
        // Security (untrusted-PR content): a committed symlink whose
        // target escapes the repo root must NOT be indexed — otherwise a
        // content rule would read out-of-tree bytes via `root.join(link)`.
        // In-tree symlinks are still followed.
        use std::os::unix::fs::symlink;
        let outside = td();
        touch(outside.path(), "secret.txt", b"TOPSECRET");
        touch(outside.path(), "secretdir/inner.txt", b"INNER");

        let root = td();
        touch(root.path(), "real.txt", b"in-tree");
        symlink(
            outside.path().join("secret.txt"),
            root.path().join("link-file"),
        )
        .unwrap();
        symlink(outside.path(), root.path().join("link-dir")).unwrap();
        symlink(root.path().join("real.txt"), root.path().join("link-in")).unwrap();

        let idx = walk(root.path(), &WalkOptions::default()).unwrap();
        let p = paths(&idx);

        assert!(
            !p.iter().any(|x| x == "link-file"),
            "escaping symlink-to-file was indexed: {p:?}"
        );
        assert!(
            !p.iter().any(|x| x.starts_with("link-dir")),
            "escaping symlink-to-dir was descended into: {p:?}"
        );
        assert!(
            p.iter().any(|x| x == "link-in"),
            "in-tree symlink should still be followed/indexed: {p:?}"
        );
        assert!(p.iter().any(|x| x == "real.txt"), "{p:?}");

        // M4: the escaping symlinks are pruned from `entries` (above) but recorded
        // on the side-list so `no_symlinks` can still flag them.
        let esc: Vec<String> = idx
            .escaping_symlinks()
            .iter()
            .map(|x| x.to_string_lossy().into_owned())
            .collect();
        assert!(
            esc.iter().any(|x| x == "link-file"),
            "escaping symlink-to-file must be on the side-list: {esc:?}"
        );
        assert!(
            esc.iter().any(|x| x == "link-dir"),
            "escaping symlink-to-dir must be on the side-list: {esc:?}"
        );
        assert!(
            !esc.iter().any(|x| x == "link-in"),
            "in-tree symlink must NOT be on the side-list: {esc:?}"
        );
    }

    #[test]
    fn fileindex_dirs_filters_files_out() {
        let idx = FileIndex::from_entries(vec![
            FileEntry {
                path: Path::new("a").into(),
                is_dir: true,
                size: 0,
            },
            FileEntry {
                path: Path::new("a/x.rs").into(),
                is_dir: false,
                size: 5,
            },
        ]);
        let dirs: Vec<_> = idx.dirs().collect();
        assert_eq!(dirs.len(), 1);
        assert_eq!(&*dirs[0].path, Path::new("a"));
    }

    #[test]
    fn fileindex_total_size_sums_files_only() {
        let idx = FileIndex::from_entries(vec![
            FileEntry {
                path: Path::new("a").into(),
                is_dir: true,
                size: 999, // dirs report 0 in `walk`, but defensively excluded here
            },
            FileEntry {
                path: Path::new("a/x.rs").into(),
                is_dir: false,
                size: 100,
            },
            FileEntry {
                path: Path::new("a/y.rs").into(),
                is_dir: false,
                size: 50,
            },
        ]);
        // total_size sums via `files()` so the directory's
        // bogus size is ignored.
        assert_eq!(idx.total_size(), 150);
    }

    #[test]
    fn fileindex_find_file_returns_match_or_none() {
        let idx = FileIndex::from_entries(vec![
            FileEntry {
                path: Path::new("a/x.rs").into(),
                is_dir: false,
                size: 0,
            },
            FileEntry {
                path: Path::new("b").into(),
                is_dir: true,
                size: 0,
            },
        ]);
        assert!(idx.find_file(Path::new("a/x.rs")).is_some());
        assert!(idx.find_file(Path::new("missing.rs")).is_none());
        // find_file filters dirs — querying a known directory
        // returns None.
        assert!(idx.find_file(Path::new("b")).is_none());
    }

    #[test]
    fn walk_excludes_dot_git_directory() {
        let tmp = td();
        touch(tmp.path(), "README.md", b"# demo\n");
        // Fake `.git/` content — should never appear in the index.
        touch(tmp.path(), ".git/config", b"[core]\n");
        touch(tmp.path(), ".git/HEAD", b"ref: refs/heads/main\n");

        let idx = walk(
            tmp.path(),
            &WalkOptions {
                respect_gitignore: false,
                extra_ignores: Vec::new(),
            },
        )
        .unwrap();

        let p = paths(&idx);
        assert!(p.contains(&"README.md".into()), "missing README.md: {p:?}");
        assert!(
            !p.iter().any(|s| s.starts_with(".git")),
            ".git was not excluded: {p:?}",
        );
    }

    #[test]
    fn walk_respects_gitignore_when_enabled() {
        let tmp = td();
        touch(tmp.path(), ".gitignore", b"target/\nignored.txt\n");
        touch(tmp.path(), "src/main.rs", b"fn main() {}\n");
        touch(tmp.path(), "target/debug/build.log", b"junk");
        touch(tmp.path(), "ignored.txt", b"junk");

        let idx = walk(
            tmp.path(),
            &WalkOptions {
                respect_gitignore: true,
                extra_ignores: Vec::new(),
            },
        )
        .unwrap();

        let p = paths(&idx);
        assert!(p.contains(&"src/main.rs".into()));
        assert!(
            !p.iter().any(|s| s.starts_with("target")),
            "target/ should be ignored: {p:?}",
        );
        assert!(
            !p.contains(&"ignored.txt".into()),
            "ignored.txt should be filtered: {p:?}",
        );
    }

    #[test]
    fn walk_includes_gitignored_paths_when_respect_gitignore_false() {
        let tmp = td();
        touch(tmp.path(), ".gitignore", b"ignored.txt\n");
        touch(tmp.path(), "ignored.txt", b"x");
        touch(tmp.path(), "kept.txt", b"y");

        let idx = walk(
            tmp.path(),
            &WalkOptions {
                respect_gitignore: false,
                extra_ignores: Vec::new(),
            },
        )
        .unwrap();
        let p = paths(&idx);
        assert!(
            p.contains(&"ignored.txt".into()),
            "respect_gitignore=false should include it: {p:?}",
        );
        assert!(p.contains(&"kept.txt".into()));
    }

    #[test]
    fn walk_applies_extra_ignores_as_excludes() {
        let tmp = td();
        touch(tmp.path(), "src/keep.rs", b"x");
        touch(tmp.path(), "vendor/skip.rs", b"y");

        let idx = walk(
            tmp.path(),
            &WalkOptions {
                respect_gitignore: false,
                extra_ignores: vec!["vendor/**".to_string()],
            },
        )
        .unwrap();
        let p = paths(&idx);
        assert!(p.contains(&"src/keep.rs".into()));
        // `vendor/**` excludes the contents but the dir entry
        // itself may still appear; the rule layer's `path_scope`
        // covers the dir-vs-file distinction. What matters here
        // is that no FILE under vendor/ was indexed.
        let file_paths: Vec<&FileEntry> = idx.files().collect();
        assert!(
            !file_paths.iter().any(|e| e.path.starts_with("vendor")),
            "no file under vendor/ should be indexed: {p:?}",
        );
    }

    #[test]
    fn walk_invalid_extra_ignore_pattern_surfaces_error() {
        let tmp = td();
        touch(tmp.path(), "a.txt", b"x");
        let err = walk(
            tmp.path(),
            &WalkOptions {
                respect_gitignore: false,
                extra_ignores: vec!["[unterminated".to_string()],
            },
        );
        assert!(err.is_err(), "bad pattern should fail: {err:?}");
    }

    #[test]
    fn walk_emits_files_with_correct_size() {
        let tmp = td();
        touch(tmp.path(), "a.txt", &[0u8; 1024]);
        let idx = walk(tmp.path(), &WalkOptions::default()).unwrap();
        let entry = idx
            .files()
            .find(|e| &*e.path == Path::new("a.txt"))
            .expect("a.txt entry");
        assert_eq!(entry.size, 1024);
        assert!(!entry.is_dir);
    }

    #[test]
    fn default_walk_options_respects_gitignore_and_no_extra_ignores() {
        let opts = WalkOptions::default();
        assert!(opts.respect_gitignore);
        assert!(opts.extra_ignores.is_empty());
    }

    #[test]
    fn walk_output_is_deterministic_across_runs() {
        // Parallel walker scheduling order is non-deterministic;
        // the deterministic post-sort by relative path is what
        // makes snapshot tests + formatters stable. Two runs over
        // the same tree must produce byte-identical FileIndex
        // outputs — guards against a forgotten sort.
        let tmp = td();
        for i in 0..50 {
            touch(
                tmp.path(),
                &format!("dir_{}/file_{i}.rs", i % 5),
                b"// hello\n",
            );
        }
        let opts = WalkOptions::default();
        let a = walk(tmp.path(), &opts).unwrap();
        let b = walk(tmp.path(), &opts).unwrap();
        assert_eq!(paths(&a), paths(&b));
    }

    #[test]
    fn walk_output_is_alphabetically_sorted() {
        // The post-sort uses path-natural ordering. We don't
        // depend on the exact ordering — just that the output IS
        // sorted, in some total order over PathBuf, so callers
        // can rely on consecutive runs returning the same shape.
        let tmp = td();
        touch(tmp.path(), "z.txt", b"z");
        touch(tmp.path(), "a.txt", b"a");
        touch(tmp.path(), "m.txt", b"m");
        touch(tmp.path(), "sub/b.txt", b"b");
        touch(tmp.path(), "sub/a.txt", b"a");

        let idx = walk(tmp.path(), &WalkOptions::default()).unwrap();
        let actual: Vec<_> = idx.entries.iter().map(|e| e.path.clone()).collect();
        let mut expected = actual.clone();
        expected.sort_unstable();
        assert_eq!(actual, expected, "walker output must be path-sorted");
    }

    #[test]
    fn walk_handles_thousand_files() {
        // Concurrency stress: enough files to land entries on
        // most worker threads on multi-core hosts. Asserts (a)
        // the count is exactly N and (b) the post-sort produces
        // a stable, total ordering matching what we'd compute
        // by sorting a manual list of expected paths.
        let tmp = td();
        let n = 1_000usize;
        for i in 0..n {
            touch(tmp.path(), &format!("d{}/f{i:04}.txt", i % 16), b"x");
        }
        let idx = walk(tmp.path(), &WalkOptions::default()).unwrap();

        let file_paths: Vec<_> = idx.files().map(|e| e.path.clone()).collect();
        assert_eq!(
            file_paths.len(),
            n,
            "expected {n} files, got {}",
            file_paths.len(),
        );

        let mut expected = file_paths.clone();
        expected.sort_unstable();
        assert_eq!(
            file_paths, expected,
            "concurrent walker output must remain path-sorted",
        );
    }

    // ── v0.9.8: parent_to_children + descendants_of ─────────────

    /// Build a synthetic [`FileIndex`] with explicit `(path, is_dir)`
    /// entries — sidesteps the filesystem walker so the
    /// `children_of` / `descendants_of` tests can target exact tree
    /// shapes without per-test tempdir scaffolding.
    fn synthetic_index(entries: &[(&str, bool)]) -> FileIndex {
        let entries = entries
            .iter()
            .map(|(p, is_dir)| FileEntry {
                path: Arc::<Path>::from(Path::new(p)),
                is_dir: *is_dir,
                size: 0,
            })
            .collect();
        FileIndex::from_entries(entries)
    }

    #[test]
    fn children_of_empty_index_returns_empty() {
        let idx = FileIndex::default();
        assert!(idx.children_of(Path::new("anything")).is_empty());
    }

    #[test]
    fn children_of_root_with_top_level_files() {
        let idx = synthetic_index(&[("a.rs", false), ("b.rs", false), ("README.md", false)]);
        let children: Vec<&str> = idx
            .children_of(Path::new(""))
            .iter()
            .map(|&i| idx.entries[i].path.to_str().unwrap())
            .collect();
        assert_eq!(children.len(), 3);
        assert!(children.contains(&"a.rs"));
        assert!(children.contains(&"b.rs"));
        assert!(children.contains(&"README.md"));
    }

    #[test]
    fn children_of_nested_dir_returns_only_direct_children() {
        let idx = synthetic_index(&[
            ("crates", true),
            ("crates/api", true),
            ("crates/api/Cargo.toml", false),
            ("crates/api/src", true),
            ("crates/api/src/main.rs", false),
            ("crates/api/src/lib.rs", false),
            ("crates/api/src/utils.rs", false),
        ]);
        let children: Vec<&str> = idx
            .children_of(Path::new("crates/api/src"))
            .iter()
            .map(|&i| idx.entries[i].path.to_str().unwrap())
            .collect();
        assert_eq!(children.len(), 3);
        assert!(children.contains(&"crates/api/src/main.rs"));
        assert!(children.contains(&"crates/api/src/lib.rs"));
        assert!(children.contains(&"crates/api/src/utils.rs"));
    }

    #[test]
    fn children_of_dir_not_in_index_returns_empty() {
        let idx = synthetic_index(&[("a.rs", false)]);
        assert!(idx.children_of(Path::new("nonexistent/dir")).is_empty());
    }

    #[test]
    fn children_of_is_memoised() {
        let idx = synthetic_index(&[("a.rs", false), ("b.rs", false)]);
        // First call builds the index. Second call must return the
        // same slice from the cache (same pointer indicates the
        // OnceLock initialised exactly once).
        let first = idx.children_of(Path::new(""));
        let second = idx.children_of(Path::new(""));
        assert_eq!(first.as_ptr(), second.as_ptr());
    }

    #[test]
    fn file_basenames_of_filters_subdirs() {
        let idx = synthetic_index(&[
            ("pkg", true),
            ("pkg/Cargo.toml", false),
            ("pkg/README.md", false),
            ("pkg/src", true), // subdir — NOT a file basename
        ]);
        let basenames: Vec<&str> = idx.file_basenames_of(Path::new("pkg")).collect();
        assert_eq!(basenames.len(), 2);
        assert!(basenames.contains(&"Cargo.toml"));
        assert!(basenames.contains(&"README.md"));
        assert!(!basenames.contains(&"src"));
    }

    #[test]
    fn descendants_of_root_yields_all_entries_depth_first() {
        let idx = synthetic_index(&[
            ("crates", true),
            ("crates/api", true),
            ("crates/api/lib.rs", false),
            ("crates/web", true),
            ("crates/web/lib.rs", false),
            ("README.md", false),
        ]);
        let descendants: Vec<&str> = idx
            .descendants_of(Path::new(""))
            .map(|e| e.path.to_str().unwrap())
            .collect();
        // Must include every entry whose parent chain reaches root.
        // Order depends on insertion order into the parent_to_children
        // map; assert membership rather than position.
        assert_eq!(descendants.len(), 6);
        for expected in [
            "crates",
            "crates/api",
            "crates/api/lib.rs",
            "crates/web",
            "crates/web/lib.rs",
            "README.md",
        ] {
            assert!(
                descendants.contains(&expected),
                "missing {expected:?} in {descendants:?}",
            );
        }
    }

    #[test]
    fn descendants_of_nested_dir_skips_outside_subtree() {
        let idx = synthetic_index(&[
            ("crates", true),
            ("crates/api", true),
            ("crates/api/lib.rs", false),
            ("crates/web", true),
            ("crates/web/lib.rs", false),
            ("README.md", false),
        ]);
        let descendants: Vec<&str> = idx
            .descendants_of(Path::new("crates/api"))
            .map(|e| e.path.to_str().unwrap())
            .collect();
        assert_eq!(descendants, vec!["crates/api/lib.rs"]);
    }

    #[test]
    fn descendants_of_short_circuits_on_take() {
        let idx = synthetic_index(&[
            ("a", true),
            ("a/b", true),
            ("a/b/c", true),
            ("a/b/c/d", true),
            ("a/b/c/d/e.rs", false),
        ]);
        // take(2) consumes only the first two yielded entries; the
        // iterator state stops descending past that. Documents the
        // "no full materialisation" contract.
        let head: Vec<&str> = idx
            .descendants_of(Path::new(""))
            .take(2)
            .map(|e| e.path.to_str().unwrap())
            .collect();
        assert_eq!(head.len(), 2);
    }

    #[test]
    fn children_of_independent_index_caches_independently() {
        // Two FileIndexes built from different entries must NOT
        // share their parent_to_children OnceLock — each instance
        // builds its own cache. Important for `--changed`-mode
        // filtered indices that live alongside the full index.
        let idx_a = synthetic_index(&[("a.rs", false)]);
        let idx_b = synthetic_index(&[("b.rs", false)]);
        let a_children = idx_a.children_of(Path::new(""));
        let b_children = idx_b.children_of(Path::new(""));
        assert_eq!(a_children.len(), 1);
        assert_eq!(b_children.len(), 1);
        let a_path = idx_a.entries[a_children[0]].path.to_str().unwrap();
        let b_path = idx_b.entries[b_children[0]].path.to_str().unwrap();
        assert_eq!(a_path, "a.rs");
        assert_eq!(b_path, "b.rs");
    }

    #[test]
    fn children_of_only_indexes_walker_known_dirs() {
        // The walker emits both files AND dirs (per the existing
        // FileEntry::is_dir field). children_of indexes by parent
        // path regardless of whether the parent itself is a known
        // entry — so a deep tree where intermediate dirs aren't
        // explicitly in entries still indexes correctly.
        let idx = synthetic_index(&[("deep/nested/a.rs", false), ("deep/nested/b.rs", false)]);
        let children = idx.children_of(Path::new("deep/nested"));
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn read_capped_or_skip_gates_on_the_passed_size() {
        // M3: the cap uses the size argument (the walk-time index size), not
        // the file — so a tiny, readable file "claimed" to be over the cap is
        // skipped without reading, and one under the cap is read.
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("small.txt");
        std::fs::write(&p, b"tiny").unwrap();
        assert!(
            read_capped_or_skip(&p, MAX_ANALYZE_BYTES + 1).is_none(),
            "over-cap size must skip"
        );
        assert_eq!(
            read_capped_or_skip(&p, 4).unwrap(),
            b"tiny",
            "under-cap size must read"
        );
    }

    #[cfg(unix)]
    #[test]
    fn walk_skips_a_fifo_special_file() {
        // A FIFO (named pipe) must NOT be indexed as a file: opening one
        // `O_RDONLY` blocks until a writer appears, so a per-file content rule
        // reading it would hang the whole run forever. Regular files alongside
        // it are still indexed.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("normal.txt"), b"hi").unwrap();
        let fifo = root.join("pipe.txt");
        let ok = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .is_ok_and(|s| s.success());
        if !ok || !fifo.exists() {
            eprintln!("mkfifo unavailable; skipping FIFO walk test");
            return;
        }

        let idx = walk(root, &WalkOptions::default()).unwrap();
        let names: Vec<_> = idx.entries.iter().map(|e| e.path.to_path_buf()).collect();
        assert!(
            names.iter().any(|p| p.as_os_str() == "normal.txt"),
            "regular file must still be indexed: {names:?}"
        );
        assert!(
            !names.iter().any(|p| p.as_os_str() == "pipe.txt"),
            "FIFO must be skipped, not indexed: {names:?}"
        );
    }

    #[test]
    fn read_capped_or_skip_missing_file_is_none() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(read_capped_or_skip(&tmp.path().join("nope"), 0).is_none());
    }

    #[test]
    fn read_bounded_bounds_the_actual_read_toctou() {
        // M3-F2: the read is bounded by the file's real bytes AT READ TIME, not
        // a (possibly stale) walk-time size — a file whose ACTUAL size exceeds
        // the cap is skipped, so concurrent growth past the cap can't force an
        // unbounded read. read_capped_or_skip delegates to this; the internal
        // cap is 256 MiB (a fixture that large is infeasible), so the bound is
        // exercised here with a small cap. The `size_hint` arg is strictly a
        // preallocation hint: a wrong hint must never change what is accepted.
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("grew.txt");
        std::fs::write(&p, vec![b'x'; 10]).unwrap();
        assert!(
            // Hint LIES that the 10-byte file fits the cap (4). The take(cap+1)
            // bound, not the hint, decides — the over-cap file is still skipped.
            read_bounded(&p, 4, 4).is_none(),
            "actual size over the cap must skip (never trust the size hint)"
        );
        assert_eq!(
            // Hint understates (0 = no preallocation); the whole file still reads.
            read_bounded(&p, 100, 0).unwrap().len(),
            10,
            "under the cap reads the whole file regardless of the hint"
        );
        assert!(
            read_bounded(&tmp.path().join("nope"), 100, 50).is_none(),
            "missing file is None"
        );
    }
}
