use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use ignore::{
    ParallelVisitor, ParallelVisitorBuilder, WalkBuilder, WalkState, overrides::OverrideBuilder,
};

use crate::error::{Error, Result};

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
    path_set: OnceLock<HashSet<Arc<Path>>>,
    parent_to_children: OnceLock<HashMap<Arc<Path>, Vec<usize>>>,
}

impl FileIndex {
    /// Construct a [`FileIndex`] from raw entries. Equivalent to
    /// `FileIndex { entries, ..Default::default() }` but spelled
    /// out so test/bench fixtures don't have to know about the
    /// internal lazy `path_set` field.
    pub fn from_entries(entries: Vec<FileEntry>) -> Self {
        Self {
            entries,
            path_set: OnceLock::new(),
            parent_to_children: OnceLock::new(),
        }
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
    /// cycle check. The walker (`crate::walk`) excludes symlinks
    /// by default, so the entries vec is acyclic by construction;
    /// adding a per-step cycle check would cost ~10 ns per yielded
    /// entry for a guarantee that's already established at
    /// walker time.
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
    let builder = build_walk_builder(root, opts)?;

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
    Ok(FileIndex::from_entries(entries))
}

/// Build the `ignore::WalkBuilder` we run today. Pure factor-out
/// of the original `walk()` body's setup half so both the
/// sequential test path and the parallel runtime path stay in
/// sync.
fn build_walk_builder(root: &Path, opts: &WalkOptions) -> Result<WalkBuilder> {
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
    Ok(builder)
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
}
