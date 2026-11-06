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
/// NOTE (bytes, not tree): this bounds INPUT size, not the parsed
/// `serde_json::Value` tree a structured format expands into, which costs up to
/// ~30x the input in RSS (attribute-heavy XML; ~6-10x for JSON / dotenv). The
/// per-file structured tree is separately bounded by
/// `structured_format::MAX_STRUCTURED_BYTES` (32 MiB, ~1 GB tree worst
/// case). What remains is the AGGREGATE across the `par_iter` fan-out (several files
/// parsing at once), a concurrent-parse-memory bound tracked as a follow-up (see
/// `docs/design/format-coverage.md`, "Resource-pressure hardening").
pub const MAX_ANALYZE_BYTES: u64 = 256 * 1024 * 1024;

/// Read a walked file's bytes, first fast-rejecting any file whose walk-time
/// [`FileEntry::size`] exceeds [`MAX_ANALYZE_BYTES`] (no extra `stat`), then
/// bounding the actual read to the cap (`read_bounded`, TOCTOU-safe). The
/// per-file loops read the whole file into memory, so an uncapped read of a
/// committed multi-GB blob would OOM the process.
///
/// This convenience API intentionally folds every failure into `None`; callers
/// that determine a policy verdict MUST use [`read_for_analysis`] and surface
/// its structural failure class instead.
pub fn read_capped_or_skip(path: &Path, size: u64) -> Option<Vec<u8>> {
    match read_for_analysis(path, size) {
        AnalysisRead::Bytes(b) => Some(b),
        AnalysisRead::Missing => None,
        AnalysisRead::TooLarge(size) => {
            tracing::warn!(
                path = %path.display(),
                size,
                cap = MAX_ANALYZE_BYTES,
                "file exceeds the analysis cap"
            );
            None
        }
        AnalysisRead::Unreadable(e) => {
            tracing::warn!(path = %path.display(), error = %e, "skipping unreadable file");
            None
        }
    }
}

/// Outcome of [`read_for_analysis`]. Every non-byte variant prevents a policy
/// verdict and must become a `resource_read` analysis failure in evaluation
/// paths.
#[derive(Debug)]
pub enum AnalysisRead {
    /// The file's bytes (within [`MAX_ANALYZE_BYTES`]).
    Bytes(Vec<u8>),
    /// The file vanished between repository discovery and the read.
    Missing,
    /// The file exceeds the bounded whole-file analysis cap. Carries the size
    /// observed at the fast stat or `cap + 1` for a concurrent-growth read.
    TooLarge(u64),
    /// A genuine read error (permission denied, I/O).
    Unreadable(std::io::Error),
}

/// [`read_capped_or_skip`], but preserving the reason analysis could not read
/// the discovered file so policy-evaluation callers can fail loudly. It logs
/// nothing: the caller reports the failure, and only it knows whether the
/// path is one alint hides.
pub fn read_for_analysis(path: &Path, size: u64) -> AnalysisRead {
    if size > MAX_ANALYZE_BYTES {
        return AnalysisRead::TooLarge(size);
    }
    // M3-F2 (TOCTOU): the walk-time `size` above is a fast reject only — it can
    // be stale, so a file that GREW past the cap between the walk and here would
    // otherwise slip through to an unbounded read. Bound the actual read by
    // BYTES (not the trusted size) so concurrent growth can't force an OOM.
    // `size` is ALSO forwarded as the read buffer's preallocation hint — alint
    // already stat-ed it during the walk, so it costs nothing and lets the read
    // finish in one syscall (see `read_bounded`).
    read_bounded_classified(path, MAX_ANALYZE_BYTES, size)
}

/// Read a whole file bounded to `cap` bytes — TOCTOU-safe: the read itself
/// stops at `cap + 1` bytes, so the file's size *at read time* (not a possibly
/// stale earlier stat / walk-time size) decides.
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
#[cfg(test)]
pub(crate) fn read_bounded(path: &Path, cap: u64, size_hint: u64) -> Option<Vec<u8>> {
    match read_bounded_classified(path, cap, size_hint) {
        AnalysisRead::Bytes(b) => Some(b),
        AnalysisRead::Missing | AnalysisRead::TooLarge(_) | AnalysisRead::Unreadable(_) => None,
    }
}

/// The classifying core of `read_bounded` (see its docs for the TOCTOU bound
/// and the preallocation hint).
fn read_bounded_classified(path: &Path, cap: u64, size_hint: u64) -> AnalysisRead {
    use std::io::Read as _;
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return AnalysisRead::Missing,
        Err(e) => return AnalysisRead::Unreadable(e),
    };
    let prealloc = usize::try_from(size_hint.min(cap.saturating_add(1))).unwrap_or(0);
    let mut buf = Vec::with_capacity(prealloc);
    match file.take(cap.saturating_add(1)).read_to_end(&mut buf) {
        Ok(_) if u64::try_from(buf.len()).is_ok_and(|n| n > cap) => {
            tracing::warn!(
                path = %path.display(),
                cap,
                "file grew past the analysis cap"
            );
            AnalysisRead::TooLarge(cap.saturating_add(1))
        }
        Ok(_) => AnalysisRead::Bytes(buf),
        Err(e) => {
            tracing::debug!(path = %path.display(), error = %e, "unreadable file");
            AnalysisRead::Unreadable(e)
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
/// `file_entry_indices` is a lazy `HashMap<Arc<Path>, usize>` over file entries.
/// Built once on the first exact-file lookup and re-used for both existence and
/// metadata queries. `path_set` retains the public set view lazily, but internal
/// callers do not build a second file index. Cross-file rules that ask exact-path
/// questions use the map instead of scanning every entry. At 1M files in a
/// 5,000-package monorepo, this turns the fan-out shape from O(D × N) =
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
/// Shared repository-relative file index (see `FileIndex::tree_files`).
type SharedFileIndex = Arc<HashMap<Arc<Path>, usize>>;
/// Shared repository-relative path set including directories.
type SharedPathSet = Arc<HashSet<Arc<Path>>>;

#[derive(Debug, Default)]
pub struct FileIndex {
    pub entries: Vec<FileEntry>,
    /// Symlinks whose target ESCAPES the repo root. Pruned from `entries` by the
    /// walk (so no content rule can `root.join()` through them to read out-of-
    /// tree bytes) but recorded here so `no_symlinks` can still flag them (M4).
    /// Relative paths, sorted + deduped. Empty for a fixture-built index.
    escaping_symlinks: Vec<Arc<Path>>,
    path_set: OnceLock<SharedPathSet>,
    all_path_set: OnceLock<SharedPathSet>,
    /// Exact file path → position in `entries`, shared with changed-file indexes
    /// when they need whole-tree exact-file lookups.
    file_entry_indices: OnceLock<SharedFileIndex>,
    /// For a `--changed`-filtered index: the FULL tree's exact-file index,
    /// shared (not copied) from the index it was filtered from, so a lookup about
    /// the repository rather than the iterated files (a
    /// `scope_filter.has_ancestor` marker) still sees unchanged files. Unset on a
    /// full index, where its own index represents the tree.
    tree_files: OnceLock<SharedFileIndex>,
    parent_to_children: OnceLock<HashMap<Arc<Path>, Vec<usize>>>,
    /// Per-`since`-ref diff sets backing `scope_filter.changed_since:`.
    /// Populated once by the engine before per-file dispatch after repository
    /// availability has been established. An unavailable repository routes a
    /// typed analysis failure before any predicate reads this cache. Read
    /// lock-free in the hot loop after `set`.
    changed_paths: OnceLock<HashMap<String, HashSet<std::path::PathBuf>>>,
    /// Per-predicate manifest-derived path sets backing
    /// `scope_filter.include_manifest_paths:` / `exclude_manifest_paths:`.
    /// Keyed by canonical predicate identity and populated before dispatch.
    manifest_paths: OnceLock<HashMap<String, crate::scope_filter::ManifestSet>>,
    /// Structural failures encountered while resolving the same predicate keys.
    /// Kept separate from the hot membership map: `ScopeFilter::matches` stays a
    /// set lookup, while the engine routes failures once per active owning rule.
    manifest_failures: OnceLock<HashMap<String, crate::scope_filter::ManifestFailure>>,
    /// Evaluated `facts:` values, cached so repeated [`Engine::run_for_file`]
    /// calls (the LSP per-keystroke path) don't re-scan the tree.
    /// ASSUMPTION: a given index is evaluated by one engine's fact set
    /// for its lifetime (true in alint — an index is walked fresh per
    /// run / per LSP session). Read lock-free after `set`.
    facts: OnceLock<crate::facts::FactValues>,
    /// Run-scoped relation read and decode caches. Enabled only when at least
    /// one configured rule consumes the relation layer, so a relation-free run
    /// keeps the direct read path and allocates no cache entries. Filtered
    /// indexes share the full index's caches.
    reads: OnceLock<std::sync::Arc<crate::relation::ReadCache>>,
    decodes: OnceLock<std::sync::Arc<crate::relation::DecodeCache>>,
}

impl FileIndex {
    /// Construct a [`FileIndex`] from raw entries. Equivalent to
    /// `FileIndex { entries, ..Default::default() }` but spelled
    /// out so test/bench fixtures don't have to know about the
    /// internal lazy indexes.
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
            all_path_set: OnceLock::new(),
            file_entry_indices: OnceLock::new(),
            tree_files: OnceLock::new(),
            parent_to_children: OnceLock::new(),
            changed_paths: OnceLock::new(),
            manifest_paths: OnceLock::new(),
            manifest_failures: OnceLock::new(),
            facts: OnceLock::new(),
            reads: OnceLock::new(),
            decodes: OnceLock::new(),
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

    /// The run-scoped relation read cache, if a relation consumer enabled it.
    ///
    /// The ordinary per-file path checks this once per file and reads directly
    /// when it is `None`; it allocates no cache entry or retained bytes, so a
    /// relation-free run is unchanged. Hidden from the docs with the relation
    /// layer it returns, as are the next two methods.
    #[doc(hidden)]
    #[must_use]
    pub fn read_cache(&self) -> Option<&crate::relation::ReadCache> {
        self.reads.get().map(std::sync::Arc::as_ref)
    }

    /// The run-scoped relation decode cache, if a relation consumer enabled it.
    #[doc(hidden)]
    #[must_use]
    pub fn decode_cache(&self) -> Option<&crate::relation::DecodeCache> {
        self.decodes.get().map(std::sync::Arc::as_ref)
    }

    /// Enable both relation caches. Idempotent: every caller shares the first
    /// pair, so two relation rules cannot decode one `(adapter, resource)` pair
    /// twice. The caches belong to this index's snapshot; a caller that
    /// re-walks a changed tree gets a fresh index and fresh caches.
    #[doc(hidden)]
    pub fn enable_relation_caches(&self) {
        self.reads
            .get_or_init(|| std::sync::Arc::new(crate::relation::ReadCache::new()));
        self.decodes
            .get_or_init(|| std::sync::Arc::new(crate::relation::DecodeCache::new()));
    }

    /// Share this index's relation caches with a filtered index built from it.
    pub(crate) fn share_relation_caches(&self, other: &Self) {
        if let Some(cache) = self.reads.get() {
            let _ = other.reads.set(std::sync::Arc::clone(cache));
        }
        if let Some(cache) = self.decodes.get() {
            let _ = other.decodes.set(std::sync::Arc::clone(cache));
        }
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
    #[must_use]
    pub(crate) fn manifest_paths(
        &self,
        cache_key: &str,
    ) -> Option<&crate::scope_filter::ManifestSet> {
        self.manifest_paths.get()?.get(cache_key)
    }

    /// Look up a structural manifest-resolution failure by predicate cache key.
    #[must_use]
    pub(crate) fn manifest_failure(
        &self,
        cache_key: &str,
    ) -> Option<&crate::scope_filter::ManifestFailure> {
        self.manifest_failures.get()?.get(cache_key)
    }

    /// `true` once the engine has populated both manifest caches.
    #[must_use]
    pub fn manifest_paths_initialized(&self) -> bool {
        self.manifest_paths.get().is_some() && self.manifest_failures.get().is_some()
    }

    /// Populate manifest path sets and failures once before parallel dispatch.
    pub(crate) fn set_manifest_paths(
        &self,
        paths: HashMap<String, crate::scope_filter::ManifestSet>,
        failures: HashMap<String, crate::scope_filter::ManifestFailure>,
    ) {
        let _ = self.manifest_paths.set(paths);
        let _ = self.manifest_failures.set(failures);
    }

    #[must_use]
    pub(crate) fn manifest_paths_map(
        &self,
    ) -> Option<&HashMap<String, crate::scope_filter::ManifestSet>> {
        self.manifest_paths.get()
    }

    #[must_use]
    pub(crate) fn manifest_failures_map(
        &self,
    ) -> Option<&HashMap<String, crate::scope_filter::ManifestFailure>> {
        self.manifest_failures.get()
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
        self.file_path_set_arc()
    }

    fn file_path_set_arc(&self) -> &SharedPathSet {
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
            Arc::new(set)
        })
    }

    fn file_entry_indices_arc(&self) -> &SharedFileIndex {
        self.file_entry_indices.get_or_init(|| {
            #[cfg(debug_assertions)]
            let start = std::time::Instant::now();
            let mut indices = HashMap::with_capacity(self.entries.len());
            for (index, entry) in self.entries.iter().enumerate() {
                if !entry.is_dir {
                    // A walked index has unique paths. Retaining the first entry
                    // also preserves `find_file`'s historical result for a
                    // hand-built index containing duplicates.
                    indices.entry(Arc::clone(&entry.path)).or_insert(index);
                }
            }
            trace_index_build!("file_entry_indices", start, self.entries.len());
            Arc::new(indices)
        })
    }

    fn all_path_set_arc(&self) -> &SharedPathSet {
        self.all_path_set.get_or_init(|| {
            #[cfg(debug_assertions)]
            let start = std::time::Instant::now();
            let set = self
                .entries
                .iter()
                .map(|entry| Arc::clone(&entry.path))
                .collect();
            trace_index_build!("all_path_set", start, self.entries.len());
            Arc::new(set)
        })
    }

    /// Share `full`'s exact-file index into this (`--changed`-filtered) index, so
    /// [`Self::contains_file_in_tree`] answers about the whole repository.
    /// Engine-only, once per run, and only when a rule needs it (a
    /// `scope_filter.has_ancestor` marker lookup); a no-op if already set. The
    /// map is reference-counted, not copied, but building it costs a pass over
    /// the full index, which a run without such lookups should not pay.
    pub(crate) fn share_tree_files_from(&self, full: &FileIndex) {
        let _ = self
            .tree_files
            .set(Arc::clone(full.file_entry_indices_arc()));
    }

    /// Does `rel` exist as a file anywhere in the repository? On a full index this
    /// is [`Self::contains_file`]; on a `--changed`-filtered index that shares the
    /// tree's file set it also sees unchanged files. Use it for lookups about the
    /// tree (ancestor markers), never for deciding which files to iterate.
    pub fn contains_file_in_tree(&self, rel: &Path) -> bool {
        match self.tree_files.get() {
            Some(files) => files.contains_key(rel),
            None => self.contains_file(rel),
        }
    }

    /// O(1) "does this exact relative path exist as a file?"
    /// query. Triggers the lazy build of the file-entry index on first call. Use
    /// this instead of iterating `files()` whenever a rule needs to check a
    /// fully-qualified path — at scale, the hash lookup is several orders of
    /// magnitude faster.
    pub fn contains_file(&self, rel: &Path) -> bool {
        self.file_entry_indices_arc().contains_key(rel)
    }

    /// O(1) "does this exact relative path exist as an indexed file or
    /// directory?" query. This is the path-resolution counterpart to
    /// [`Self::contains_file`]: documentation links and other repository
    /// references may legitimately target either kind.
    pub fn contains_path(&self, rel: &Path) -> bool {
        self.all_path_set_arc().contains(rel)
    }

    /// Find a file entry by its exact relative path in O(1) average time.
    ///
    /// The path → entry index is built lazily on first use. Like
    /// [`Self::contains_file`], this only sees non-directory entries admitted by
    /// the walker, preserving ignore and repository-boundary semantics.
    pub fn find_file(&self, rel: &Path) -> Option<&FileEntry> {
        let entry_index = *self.file_entry_indices_arc().get(rel)?;
        self.entries.get(entry_index)
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
            // Dir path -> its entry's Arc, built in one pass so promoting a
            // parent to a map key below is an O(1) probe. The previous per-parent
            // linear `entries.iter().find(..)` made this build O(dirs x N) despite
            // the documented O(N): 20k dirs took ~40s in a debug build
            // (audit 2026-10 finding 6). First occurrence wins, as `find` did.
            let mut dir_arcs: HashMap<&Path, &Arc<Path>> = HashMap::new();
            for e in self.entries.iter().filter(|e| e.is_dir) {
                dir_arcs.entry(&*e.path).or_insert(&e.path);
            }
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
                let key: Arc<Path> = dir_arcs
                    .get(parent)
                    .map_or_else(|| Arc::<Path>::from(parent), |a| Arc::clone(a));
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
/// The walk's override set: `.git/` plus every config `ignore:` pattern, each
/// an exclusion.
fn build_overrides(root: &Path, opts: &WalkOptions) -> Result<ignore::overrides::Override> {
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
    overrides_builder
        .build()
        .map_err(|e| Error::Other(format!("failed to build overrides: {e}")))
}

/// The `info/exclude` file the walk reads for a directory `dir` that holds a
/// repository: `<dir>/.git/info/exclude` when `.git` is a directory, or, when
/// it is a linked worktree's `.git` file (`gitdir: <dir>`), `info/exclude` in
/// the common git directory `<dir>/commondir` names, since a worktree shares
/// its repository's excludes. Mirrors `ignore`'s `resolve_git_commondir`
/// (0.4.34), including reading no environment variables and a relative
/// `gitdir:` resolving against the working directory. `None` when the walk
/// would read no exclude file.
fn git_exclude_file(dir: &Path) -> Option<PathBuf> {
    use std::io::BufRead as _;
    let first_line = |path: &Path| -> Option<String> {
        let file = std::fs::File::open(path).ok()?;
        std::io::BufReader::new(file).lines().next()?.ok()
    };
    let dot_git = dir.join(".git");
    if !std::fs::metadata(&dot_git).is_ok_and(|meta| meta.is_file()) {
        return Some(dot_git.join("info/exclude"));
    }
    let git_dir = PathBuf::from(first_line(&dot_git)?.strip_prefix("gitdir: ")?);
    let common = first_line(&git_dir.join("commondir"))?;
    let common = if common.starts_with('.') {
        git_dir.join(common)
    } else {
        PathBuf::from(common)
    };
    Some(common.join("info/exclude"))
}

/// The names in `dir`'s listing. The walk finds the ignore files and
/// repository markers of a directory it lists (the root and every directory
/// below it) by exact name in that listing, as `ignore`'s
/// `collect_ignore_files` does, so on a case-insensitive filesystem
/// `.GITIGNORE` is not a `.gitignore`. Empty when `dir` cannot be listed or
/// does not exist yet.
fn listed_names(dir: &Path) -> HashSet<std::ffi::OsString> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok().map(|entry| entry.file_name()))
                .collect()
        })
        .unwrap_or_default()
}

/// Whether `dir` holds a repository: a `.git` directory, the `.git` file of a
/// linked worktree or submodule, or a `.jj` directory, the markers `ignore`
/// looks for under `require_git`. In a listed directory (`names`, from
/// [`listed_names`]) the marker must also appear under its exact name.
fn holds_repository(dir: &Path, names: Option<&HashSet<std::ffi::OsString>>) -> bool {
    let holds = |marker: &str| {
        names.is_none_or(|names| names.contains(std::ffi::OsStr::new(marker)))
            && dir.join(marker).exists()
    };
    holds(".git") || holds(".jj")
}

/// Whether `root` lies in a repository: it, or a directory above it, holds one
/// ([`holds_repository`]; the root is listed, those above it are not). The
/// walk then reads ignore files up to that repository's top and no further;
/// outside a repository, the root is the boundary.
fn in_repository(root: &Path) -> bool {
    holds_repository(root, Some(&listed_names(root)))
        || root.canonicalize().is_ok_and(|root| {
            root.ancestors()
                .skip(1)
                .any(|dir| holds_repository(dir, None))
        })
}

/// An ignore matcher anchored at `dir` built from `content` as `ignore` builds
/// one from the file `file` (`GitignoreBuilder::add`): line by line, without
/// a UTF-8 BOM on the first line, stopping at a line that is not UTF-8 and
/// skipping one that is not a valid glob.
fn gitignore_from(dir: &Path, file: &Path, content: &[u8]) -> Option<ignore::gitignore::Gitignore> {
    use std::io::BufRead as _;
    let mut builder = ignore::gitignore::GitignoreBuilder::new(dir);
    for (number, line) in content.lines().enumerate() {
        let Ok(line) = line else {
            break;
        };
        let line = if number == 0 {
            line.trim_start_matches('\u{feff}')
        } else {
            &line
        };
        // A line that is not a valid glob is left out, as `ignore` does.
        let _ = builder.add_line(Some(file.to_path_buf()), line);
    }
    builder.build().ok()
}

/// The ignore files the walk reads in one directory, each anchored there.
struct DirIgnores {
    dot_ignore: Option<ignore::gitignore::Gitignore>,
    git_ignore: Option<ignore::gitignore::Gitignore>,
    /// The repository's `info/exclude`, when `dir` holds one (`.git`).
    exclude: Option<ignore::gitignore::Gitignore>,
    /// Whether `dir` holds a repository ([`holds_repository`]).
    repository: bool,
}

impl DirIgnores {
    /// The ignore files of `dir`. In a directory the walk lists (`listed`: the
    /// root and those below it) a file counts only under its exact name
    /// ([`listed_names`]); above the root `ignore` probes each path.
    fn read(dir: &Path, listed: bool) -> Self {
        let names = listed.then(|| listed_names(dir));
        let has = |name: &str| {
            names
                .as_ref()
                .is_none_or(|names| names.contains(std::ffi::OsStr::new(name)))
        };
        let load = |file: Option<PathBuf>| {
            let file = file.filter(|file| file.is_file())?;
            let mut builder = ignore::gitignore::GitignoreBuilder::new(dir);
            builder.add(&file);
            builder.build().ok()
        };
        Self {
            dot_ignore: load(has(".ignore").then(|| dir.join(".ignore"))),
            git_ignore: load(has(".gitignore").then(|| dir.join(".gitignore"))),
            exclude: load(has(".git").then(|| git_exclude_file(dir)).flatten()),
            repository: holds_repository(dir, names.as_ref()),
        }
    }

    /// Take the ignore files among `created` that a fix will write in this
    /// directory (`rel`, root-relative) in place of what is there now.
    /// A file created through an in-tree symlinked directory lands in the
    /// directory the link leads to, which the walk also lists under the link's
    /// name, so directories compare resolved.
    fn with_created(mut self, root: &Path, rel: &Path, created: &[(&Path, &[u8])]) -> Self {
        let dir = root.join(rel);
        let mut resolved_dir = None;
        for (path, content) in created {
            let slot = match path.file_name().and_then(std::ffi::OsStr::to_str) {
                Some(".ignore") => &mut self.dot_ignore,
                Some(".gitignore") => &mut self.git_ignore,
                _ => continue,
            };
            let parent = path.parent().unwrap_or_else(|| Path::new(""));
            if parent != rel {
                let resolved_dir = resolved_dir
                    .get_or_insert_with(|| crate::pathsafe::resolve_existing_prefix(&dir));
                if resolved_dir.is_none()
                    || crate::pathsafe::resolve_existing_prefix(&root.join(parent)) != *resolved_dir
                {
                    continue;
                }
            }
            *slot = gitignore_from(&dir, &root.join(path), content);
        }
        self
    }
}

/// Whether the walk stops at a symlink on the way to `parts` (root-relative)
/// or at `parts` itself: one that leads back to a directory on its own path, a
/// loop `ignore` reports and does not descend, or one that is broken or leads
/// out of the root, which the walk prunes (`build_walk_builder`). Only paths
/// that exist count; one that does not exist yet cannot be a symlink.
fn symlink_stops_the_walk(root: &Path, parts: &[std::path::Component<'_>]) -> bool {
    let canonical_root = root.canonicalize().ok();
    // The directories the walk has entered on the way, canonical.
    let mut entered: Vec<PathBuf> = canonical_root.iter().cloned().collect();
    for k in 1..=parts.len() {
        let dir = root.join(parts[..k].iter().collect::<PathBuf>());
        let Ok(metadata) = dir.symlink_metadata() else {
            return false;
        };
        let canonical = dir.canonicalize().ok();
        if metadata.file_type().is_symlink() {
            let Some(target) = &canonical else {
                return true;
            };
            if canonical_root
                .as_ref()
                .is_none_or(|root| !target.starts_with(root))
                || entered.contains(target)
            {
                return true;
            }
        }
        let Some(canonical) = canonical else {
            return false;
        };
        entered.push(canonical);
    }
    false
}

/// Whether [`walk`] would leave the repo-relative path `rel` out of the index
/// if it existed (as a directory when `is_dir`): excluded by the config's
/// `ignore:` / `.git`, or (with `respect_gitignore`) by an ignore file the
/// walk reads -- itself or through an excluded ancestor directory, which the
/// walk never descends into. `fix --dry-run` uses it to predict that a file a
/// fix would create stays invisible to the rule, as the real `fix` discovers
/// on its re-walk.
///
/// Mirrors `ignore` 0.4.34 as the walk configures it. Overrides come first.
/// Then each entry on the way to `rel` is matched against the ignore files of
/// the directories the walk passed through to reach it (never its own, which
/// apply only below it), nearest first, then, when the root lies in a
/// repository, those of every directory above the root, up to the filesystem
/// root. Each directory contributes its `.ignore`, its `.gitignore` and, when
/// it holds a repository (a nested one or a linked worktree too), that
/// repository's `info/exclude` (`git_exclude_file`). In a repository,
/// `.gitignore` and `info/exclude` stop at the nearest directory that holds
/// one, as git's do; `.ignore` files do not. The nearest match of each kind is
/// taken, and the kinds rank `.ignore`, `.gitignore`, `info/exclude`, then the
/// global excludes file. In a directory the walk lists (the root and below)
/// an ignore file or repository marker counts only under its exact name.
/// Separately, the walk stops at a symlinked directory that loops back, is
/// broken or leads out of the root (`symlink_stops_the_walk`). Errors building
/// a matcher read as "not ignored".
///
/// `created` lists the files the fix is about to create (root-relative path
/// and content). An ignore file among them (`.gitignore`, `.ignore`) is read
/// in its directory as the walk after the fix will read it, so a created
/// `.gitignore` of `*` leaves itself out.
#[must_use]
pub fn would_walk_skip(
    root: &Path,
    opts: &WalkOptions,
    rel: &Path,
    is_dir: bool,
    created: &[(&Path, &[u8])],
) -> bool {
    use ignore::Match;
    use ignore::gitignore::Gitignore;
    let parts: Vec<_> = rel
        .components()
        .filter(|c| !matches!(c, std::path::Component::CurDir))
        .collect();
    if parts.is_empty() {
        return false;
    }
    let overrides = build_overrides(root, opts).ok();
    // `in_tree[k]` holds the ignore files of the directory `parts[..k]` below
    // the root; `parents`, those of each directory above the (canonical) root,
    // nearest first, matched against absolute paths as the walk does. The walk
    // reads directories above the root only inside a repository (`bounded`).
    let (in_tree, canonical_root, parents, global, bounded) = if opts.respect_gitignore {
        let bounded = in_repository(root);
        let in_tree: Vec<DirIgnores> = (0..parts.len())
            .map(|k| {
                let dir: PathBuf = parts[..k].iter().collect();
                DirIgnores::read(&root.join(&dir), true).with_created(root, &dir, created)
            })
            .collect();
        let canonical_root = root.canonicalize().ok();
        let parents: Vec<DirIgnores> = canonical_root
            .iter()
            .filter(|_| bounded)
            .flat_map(|root| root.ancestors().skip(1))
            .map(|dir| DirIgnores::read(dir, false))
            .collect();
        (
            in_tree,
            canonical_root,
            parents,
            Some(Gitignore::global().0),
            bounded,
        )
    } else {
        (Vec::new(), None, Vec::new(), None, false)
    };

    // The entry `parts[..=i]`: the ignore files of `parts[..k]` for `k <= i`
    // apply to it, the nearest match of each kind winning.
    let ignored = |i: usize, is_dir: bool| -> bool {
        let prefix: PathBuf = parts[..=i].iter().collect();
        let path = root.join(&prefix);
        if let Some(ov) = &overrides {
            match ov.matched(&path, is_dir) {
                Match::Ignore(_) => return true,
                Match::Whitelist(_) => return false,
                Match::None => {}
            }
        }
        let absolute = canonical_root.as_ref().map(|root| root.join(&prefix));
        // The nearest match of one kind. A kind that `stops` is not read past
        // the nearest directory holding a repository (that directory's own
        // files still count).
        let nearest = |kind: fn(&DirIgnores) -> Option<&Gitignore>, stops: bool| {
            let in_tree = in_tree
                .get(..=i)
                .unwrap_or_default()
                .iter()
                .rev()
                .map(|dir| (dir, path.as_path()));
            let above = absolute
                .iter()
                .flat_map(|absolute| parents.iter().map(move |dir| (dir, absolute.as_path())));
            for (dir, candidate) in in_tree.chain(above) {
                let found = kind(dir).map(|gi| gi.matched(candidate, is_dir));
                if let Some(found) = found.filter(|m| !m.is_none()) {
                    return Some(found);
                }
                if stops && dir.repository {
                    break;
                }
            }
            None
        };
        let found = nearest(|d| d.dot_ignore.as_ref(), false)
            .or_else(|| nearest(|d| d.git_ignore.as_ref(), bounded))
            .or_else(|| nearest(|d| d.exclude.as_ref(), bounded))
            .or_else(|| global.as_ref().map(|gi| gi.matched(&path, is_dir)));
        matches!(found, Some(Match::Ignore(_)))
    };
    // The path itself, and every ancestor directory (an excluded directory is
    // never descended into, so nothing below it is indexed).
    symlink_stops_the_walk(root, &parts)
        || (0..parts.len()).any(|i| ignored(i, if i + 1 == parts.len() { is_dir } else { true }))
}

fn build_walk_builder(root: &Path, opts: &WalkOptions) -> Result<(WalkBuilder, EscapingSink)> {
    let mut builder = WalkBuilder::new(root);
    builder
        .standard_filters(opts.respect_gitignore)
        .hidden(false)
        .follow_links(true);
    // Ignore files stop at the repository boundary, as git's do (ADR-0028). In
    // a repository, `require_git` makes `.gitignore` and `info/exclude` stop at
    // the nearest directory holding one (the repository's top, or a nested
    // repository's), and `parents` reads the in-repository directories above
    // a root linted from a subdirectory. Outside a repository the root is the
    // boundary, and the root's own ignore files apply without git.
    if opts.respect_gitignore {
        let bounded = in_repository(root);
        builder.require_git(bounded).parents(bounded);
    }

    // Always exclude `.git/` — descending into git's internal
    // packfiles + loose objects is wasted work for every alint
    // rule (none of them target `.git/objects/*`), and it races
    // git's auto-gc / pack-rewrite on large repos. We set
    // `hidden(false)`, which would otherwise skip `.git/` with
    // every other dot-directory; this override puts it back.
    builder.overrides(build_overrides(root, opts)?);

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
        if !keep
            && let Ok(rel) = entry.path().strip_prefix(&root_for_rel)
            && !rel.as_os_str().is_empty()
            && let Ok(mut v) = escaping_filter.lock()
        {
            v.push(Arc::from(rel));
        }
        keep
    });
    Ok((builder, escaping))
}

/// Whether a walk error is a PER-ENTRY condition to skip (return `Ok(None)`)
/// rather than a fatal abort of the whole walk: a dangling symlink or a file
/// that vanished mid-walk (I/O `NotFound`), a symlink cycle, or an ignore file
/// that could not be read or parsed. Any other error (permission denied on a
/// directory, ...) is left to propagate.
fn is_skippable_walk_error(root: &Path, err: &ignore::Error) -> bool {
    if err.io_error().map(std::io::Error::kind) == Some(std::io::ErrorKind::NotFound) {
        return true;
    }
    is_symlink_loop(err) || is_eloop(err) || is_ignore_file_error(root, err)
}

/// Whether every part of a walk error names a file above the root. `ignore`
/// reads the ignore files of every directory above the root before the walk
/// starts, whether or not it applies them (ADR-0029), and reports a bad line
/// in one, or a `.git` file it cannot read, as a walk error naming that file
/// (several make a `Partial`), where an ignore file in the tree only marks its
/// directory's entry. A bad line is dropped from its matcher either way, so
/// the walk goes on as git does: git skips a pattern it cannot parse and never
/// reads outside the repository. An error from the walk itself names an entry
/// under the root, so it is not one of these. The path decides, not the depth
/// most walk errors carry: a failed stat on a followed symlink arrives as a
/// `WithPath` around the `WithDepth`.
fn is_ignore_file_error(root: &Path, err: &ignore::Error) -> bool {
    match err {
        // Every entry the walk yields is under the root, so a path outside it
        // is one of the files read above it.
        ignore::Error::WithPath { path, .. } => path.strip_prefix(root).is_err(),
        ignore::Error::Partial(errs) => {
            !errs.is_empty() && errs.iter().all(|err| is_ignore_file_error(root, err))
        }
        _ => false,
    }
}

/// Whether an `ignore` walk error is (or wraps) a symlink-cycle `Loop`. The
/// `ignore` crate nests the real error under `WithPath` / `WithDepth` /
/// `WithLineNumber` / `Partial`, and `Loop` carries no `io_error()`, so a
/// straight `io_error().kind()` check misses it -- unwrap to find it. Catches
/// the ancestor-based cycles the crate detects itself (`..`, mutual links).
fn is_symlink_loop(err: &ignore::Error) -> bool {
    use ignore::Error;
    match err {
        Error::Loop { .. } => true,
        Error::WithPath { err, .. }
        | Error::WithDepth { err, .. }
        | Error::WithLineNumber { err, .. } => is_symlink_loop(err),
        Error::Partial(errs) => errs.iter().any(is_symlink_loop),
        _ => false,
    }
}

/// Whether a walk error is a kernel `ELOOP` (too many levels of symbolic links)
/// -- a DIRECT self-loop (`ln -s x x`) that the crate's ancestor tracking does
/// not flag as a `Loop`. `std::io::ErrorKind::FilesystemLoop` is still unstable
/// (rust #86442), so match the platform errno: 40 on Linux/Android, 62 on
/// macOS/*BSD. Non-unix targets have no ELOOP here (symlink cycles surface via
/// the crate's `Loop`), so this is a no-op there.
#[cfg(unix)]
fn is_eloop(err: &ignore::Error) -> bool {
    const ELOOP: i32 = if cfg!(any(target_os = "linux", target_os = "android")) {
        40
    } else {
        62
    };
    err.io_error().and_then(std::io::Error::raw_os_error) == Some(ELOOP)
}
#[cfg(not(unix))]
fn is_eloop(_err: &ignore::Error) -> bool {
    false
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
    let entry = match result {
        Ok(entry) => entry,
        Err(err) => {
            // Skip a handful of PER-ENTRY conditions rather than aborting the
            // ENTIRE run (one bad path anywhere must not make a whole repo
            // un-lintable / un-fixable):
            //   * a broken (dangling) symlink -- under `follow_links(true)` the
            //     target can't be stat'd, surfacing as I/O `NotFound`. (The link
            //     itself is not indexed, so `no_symlinks`/`file_remove` do not see
            //     it -- a known detection gap, tracked separately.) `NotFound`
            //     also covers a file that vanished mid-walk (listed then removed).
            //   * a symlink CYCLE -- `sub/up -> ..`, a self-loop, or a mutual
            //     loop. The `ignore` crate reports its own `Loop`, and the kernel
            //     reports `ELOOP` (`FilesystemLoop`); neither is `NotFound`, so
            //     before this both aborted the run with exit 2 on a single stray
            //     link. `filter_entry` already prunes the recursion; the emitted
            //     error just needs to be non-fatal too.
            //   * a bad ignore file above the root (`a\` in a parent
            //     directory's `.gitignore`), which git never reads. The same line
            //     in the tree's own `.gitignore` was already tolerated.
            // Any OTHER error (permission denied, ...) still propagates and stops
            // the walk, matching the prior contract.
            if is_skippable_walk_error(root, &err) {
                return Ok(None);
            }
            return Err(err.into());
        }
    };
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
#[path = "walker_tests.rs"]
mod tests;
