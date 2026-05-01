use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use ignore::{
    ParallelVisitor, ParallelVisitorBuilder, WalkBuilder, WalkState, overrides::OverrideBuilder,
};

use crate::error::{Error, Result};

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
#[derive(Debug, Default)]
pub struct FileIndex {
    pub entries: Vec<FileEntry>,
    path_set: OnceLock<HashSet<Arc<Path>>>,
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
            self.entries
                .iter()
                .filter(|e| !e.is_dir)
                .map(|e| Arc::clone(&e.path))
                .collect()
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
}
