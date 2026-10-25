//! Run-scoped read and decode caches (v0.21 R2).
//!
//! The engine reads a file once for every per-file rule (ADR-0003) and the
//! relation layer must extend that guarantee. A [`ReadCache`] maps a
//! repository path to one [`ResourceRead`]; a [`DecodeCache`] maps
//! `(adapter id, repository path)` to one [`AdapterOutput`]. Both use a
//! per-key [`OnceLock`] obtained under a short map lock, so concurrent
//! consumers share one initializer instead of racing to decode twice.

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use super::adapter::{Adapter, AdapterError, AdapterOutput};
use super::limits::Limits;
use super::resource::{Resource, ResourceRead};

/// One resource read, cached for the whole run.
///
/// Only paths a relation consumer marks with [`ReadCache::mark_interest`] are
/// retained. The ordinary per-file loop still reads through this cache (so a
/// relation and a per-file rule share one read), but a file no relation asked
/// for is read directly and immediately dropped, so a relation-free file set
/// cannot turn into gigabytes of retained bytes at 1M files.
#[derive(Default)]
pub struct ReadCache {
    slots: Mutex<HashMap<PathBuf, Arc<OnceLock<Arc<ResourceRead>>>>>,
    interested: Mutex<HashSet<PathBuf>>,
    reads: AtomicUsize,
    hits: AtomicUsize,
}

impl std::fmt::Debug for ReadCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print cached bytes or error strings: `FileIndex` derives
        // `Debug`, and a log line must not carry repository contents.
        f.debug_struct("ReadCache")
            .field("entries", &self.slots.lock().map_or(0, |slots| slots.len()))
            .field(
                "interested",
                &self.interested.lock().map_or(0, |set| set.len()),
            )
            .field("reads", &self.reads.load(Ordering::Relaxed))
            .field("hits", &self.hits.load(Ordering::Relaxed))
            .finish()
    }
}

impl ReadCache {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Read `resource` at most once. `size_hint` is the walk-time file size for
    /// an index entry (a preallocation hint only), or zero for a literal path
    /// outside the index.
    ///
    /// A literal path is symlink-confined before the read; an index entry was
    /// already pruned by the walker, so it takes the cheaper path.
    pub fn read(&self, root: &Path, resource: &Resource, size_hint: u64) -> Arc<ResourceRead> {
        let literal = matches!(resource.origin(), super::resource::Origin::Literal);
        let relative = Path::new(resource.path().as_str());
        self.mark_interest(relative);
        self.read_inner(root, relative, size_hint, literal)
    }

    /// Mark a resource as one a relation consumer will read, so the read is
    /// retained for the per-file pass. Relation rules call this before reading
    /// an index resource; [`read`](Self::read) marks a literal resource
    /// automatically.
    pub fn mark_interest(&self, relative: &Path) {
        let mut interested = self.interested.lock().expect("read cache interest lock");
        interested.insert(relative.to_path_buf());
        #[cfg(windows)]
        {
            interested.insert(cache_key(relative));
            if let Some(text) = relative.to_str() {
                interested.insert(PathBuf::from(text.replace('/', "\\")));
            }
        }
    }

    fn is_interested(&self, relative: &Path) -> bool {
        let interested = self.interested.lock().expect("read cache interest lock");
        if interested.contains(relative) {
            return true;
        }
        // On Unix the walked separator is already `/`, so the exact lookup
        // above is the complete check. Windows may hand us `a\b` for a
        // relation path recorded as `a/b`; fall back to the normalized key.
        #[cfg(windows)]
        {
            interested.contains(&cache_key(relative))
        }
        #[cfg(not(windows))]
        {
            false
        }
    }

    /// [`read`](Self::read) for a walked index path that may not be
    /// representable as a [`RelPath`](super::RelPath) (a non-UTF-8 filename on
    /// Unix). Index entries are symlink-pruned by the walk and are regular
    /// files, so this skips the filesystem containment and file-type checks.
    ///
    /// Crate-internal: the engine's per-file loop calls it with an index path.
    /// Relation rules use [`read`](Self::read) with a [`Resource`], which
    /// applies the literal-path checks.
    ///
    /// Without [`mark_interest`](Self::mark_interest) the bytes are returned
    /// directly and not retained.
    pub(crate) fn read_index_path(
        &self,
        root: &Path,
        relative: &Path,
        size_hint: u64,
    ) -> Arc<ResourceRead> {
        if !self.is_interested(relative) {
            self.reads.fetch_add(1, Ordering::Relaxed);
            return Arc::new(read_once(root, relative, size_hint, false));
        }
        self.read_inner(root, relative, size_hint, false)
    }

    fn read_inner(
        &self,
        root: &Path,
        relative: &Path,
        size_hint: u64,
        literal: bool,
    ) -> Arc<ResourceRead> {
        let key = cache_key(relative);
        let slot = {
            let mut slots = self.slots.lock().expect("read cache lock");
            if let Some(slot) = slots.get(&key) {
                Arc::clone(slot)
            } else {
                let slot = Arc::new(OnceLock::new());
                slots.insert(key, Arc::clone(&slot));
                slot
            }
        };
        let already_present = slot.get().is_some();
        let value = slot
            .get_or_init(|| {
                self.reads.fetch_add(1, Ordering::Relaxed);
                Arc::new(read_once(root, relative, size_hint, literal))
            })
            .clone();
        if already_present {
            self.hits.fetch_add(1, Ordering::Relaxed);
        }
        value
    }

    /// How many filesystem reads this cache performed. Test/diagnostic only.
    #[must_use]
    pub fn reads(&self) -> usize {
        self.reads.load(Ordering::Relaxed)
    }

    /// How many calls reused an existing entry. Test/diagnostic only.
    #[must_use]
    pub fn hits(&self) -> usize {
        self.hits.load(Ordering::Relaxed)
    }
}

/// Normalize a walked or literal path to one `/`-separated key, so a Windows
/// `Path` (`a\b`) and a relation path (`a/b`) name the same cache entry. A
/// non-UTF-8 component falls back to the OS path, which still distinguishes
/// distinct names.
fn cache_key(relative: &Path) -> PathBuf {
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => match part.to_str() {
                Some(part) => parts.push(part),
                None => return relative.to_path_buf(),
            },
            // `FileIndex` never stores these, but a literal resource path is
            // already normalized; fall back rather than guess.
            _ => return relative.to_path_buf(),
        }
    }
    PathBuf::from(parts.join("/"))
}

fn read_once(root: &Path, relative: &Path, size_hint: u64, literal: bool) -> ResourceRead {
    let absolute = root.join(relative);
    let mut size_hint = size_hint;
    if literal {
        if !crate::pathsafe::resolved_within_root(&absolute, root) {
            return ResourceRead::Unreadable("path escapes the repo root".to_string());
        }
        // A literal resource is config-derived and may name a FIFO, socket, or
        // device. Opening a FIFO `O_RDONLY` blocks until a writer appears, so
        // reject every non-regular file before `read_for_analysis` opens it.
        // Index entries are already regular files, so the per-file hot path
        // skips this extra stat.
        match std::fs::metadata(&absolute) {
            Ok(metadata) if !metadata.is_file() => {
                return ResourceRead::Unreadable(format!(
                    "{} is not a regular file",
                    absolute.display()
                ));
            }
            Ok(metadata) => size_hint = metadata.len(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return ResourceRead::Missing;
            }
            Err(error) => return ResourceRead::Unreadable(error.to_string()),
        }
    }
    match crate::walker::read_for_analysis(&absolute, size_hint) {
        crate::walker::AnalysisRead::Bytes(bytes) => {
            ResourceRead::Present(Arc::from(bytes.into_boxed_slice()))
        }
        crate::walker::AnalysisRead::Missing => ResourceRead::Missing,
        crate::walker::AnalysisRead::TooLarge(size) => ResourceRead::TooLarge(size),
        crate::walker::AnalysisRead::Unreadable(error) => {
            ResourceRead::Unreadable(error.to_string())
        }
    }
}

type DecodeResult = Result<Arc<AdapterOutput>, AdapterError>;
type DecodeSlots = HashMap<(String, String), Arc<OnceLock<DecodeResult>>>;

/// One adapter output, cached for the whole run.
///
/// A cache belongs to one run: the limits supplied to [`decode`](Self::decode)
/// are part of that run's context, not part of the key.
#[derive(Default)]
pub struct DecodeCache {
    slots: Mutex<DecodeSlots>,
    decodes: AtomicUsize,
}

impl std::fmt::Debug for DecodeCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Rows and diagnostics can carry repository values; never print them.
        f.debug_struct("DecodeCache")
            .field("entries", &self.slots.lock().map_or(0, |slots| slots.len()))
            .field("decodes", &self.decodes.load(Ordering::Relaxed))
            .finish()
    }
}

impl DecodeCache {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Decode `resource` through `adapter` at most once per
    /// `(adapter.id(), resource.path())` pair.
    ///
    /// The cache keys on the adapter's specification-bearing id, so two
    /// different `Extract` queries over one file do not share an entry.
    pub fn decode(
        &self,
        adapter: &dyn Adapter,
        resource: &Resource,
        bytes: &[u8],
        limits: &Limits,
    ) -> DecodeResult {
        let key = (
            adapter.id().to_string(),
            resource.path().as_str().to_string(),
        );
        let slot = {
            let mut slots = self.slots.lock().expect("decode cache lock");
            slots
                .entry(key)
                .or_insert_with(|| Arc::new(OnceLock::new()))
                .clone()
        };
        slot.get_or_init(|| {
            self.decodes.fetch_add(1, Ordering::Relaxed);
            adapter.extract(resource, bytes, limits).map(Arc::new)
        })
        .clone()
    }

    /// How many decodes this cache performed. Test/diagnostic only.
    #[must_use]
    pub fn decodes(&self) -> usize {
        self.decodes.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::Extract;
    use crate::relation::adapters::ExtractAdapter;
    use crate::relation::limits::LimitKind;
    use crate::relation::resource::Resource;
    use crate::relation::value::Value;

    fn literal(path: &str) -> Resource {
        Resource::literal(crate::relation::RelPath::parse(path).unwrap())
    }

    #[test]
    fn a_literal_non_regular_file_is_rejected_before_opening() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("a-directory")).unwrap();
        let cache = ReadCache::new();
        let read = cache.read(dir.path(), &literal("a-directory"), 0);
        assert!(
            matches!(read.as_ref(), ResourceRead::Unreadable(message) if message.contains("not a regular file")),
            "{read:?}"
        );
        #[cfg(unix)]
        {
            let _socket = std::os::unix::net::UnixListener::bind(dir.path().join("a-socket"))
                .expect("bind a test socket");
            let read = cache.read(dir.path(), &literal("a-socket"), 0);
            assert!(
                matches!(read.as_ref(), ResourceRead::Unreadable(message) if message.contains("not a regular file")),
                "{read:?}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_literal_fifo_is_rejected_without_blocking() {
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("a-fifo");
        let status = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("run mkfifo");
        assert!(status.success(), "mkfifo failed");
        let root = dir.path().to_path_buf();
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let cache = ReadCache::new();
            let read = cache.read(&root, &literal("a-fifo"), 0);
            let _ = sender.send(read);
        });
        let read = receiver
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("a FIFO must be rejected before it can block the read");
        assert!(
            matches!(read.as_ref(), ResourceRead::Unreadable(message) if message.contains("not a regular file")),
            "{read:?}"
        );
    }

    #[test]
    fn cache_debug_never_prints_cached_bytes() {
        let cache = ReadCache::new();
        let resource = literal("a.txt");
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"top-secret-value").unwrap();
        let _ = cache.read(dir.path(), &resource, 0);
        let rendered = format!("{cache:?}");
        assert!(!rendered.contains("top-secret-value"), "{rendered}");

        let decodes = DecodeCache::new();
        let adapter = ExtractAdapter::new(Extract::WholeFile);
        let _ = decodes
            .decode(&adapter, &resource, b"top-secret-value", &Limits::default())
            .unwrap();
        let rendered = format!("{decodes:?}");
        assert!(!rendered.contains("top-secret-value"), "{rendered}");
    }

    #[test]
    fn file_index_debug_does_not_print_cached_bytes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"top-secret-value").unwrap();
        let index = crate::walker::FileIndex::from_entries(vec![crate::walker::FileEntry {
            path: std::sync::Arc::from(std::path::Path::new("a.txt")),
            is_dir: false,
            size: 16,
        }]);
        index.enable_relation_caches();
        let cache = index.read_cache().expect("cache enabled");
        let _ = cache.read(dir.path(), &Resource::for_test(0, "a.txt"), 16);
        let rendered = format!("{index:?}");
        assert!(!rendered.contains("top-secret-value"), "{rendered}");
    }

    #[test]
    fn a_read_happens_once_per_path() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"hello").unwrap();
        let cache = ReadCache::new();
        let resource = literal("a.txt");
        let first = cache.read(dir.path(), &resource, 0);
        let second = cache.read(dir.path(), &resource, 0);
        assert_eq!(cache.reads(), 1);
        assert_eq!(
            first.as_ref(),
            &ResourceRead::Present(Arc::<[u8]>::from(&b"hello"[..]))
        );
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn index_paths_use_the_same_cache_without_a_symlink_probe() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"hello").unwrap();
        let cache = ReadCache::new();
        cache.mark_interest(std::path::Path::new("a.txt"));
        let first = cache.read_index_path(dir.path(), std::path::Path::new("a.txt"), 0);
        let second = cache.read_index_path(dir.path(), std::path::Path::new("a.txt"), 0);
        assert_eq!(cache.reads(), 1);
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn an_uninteresting_index_path_is_not_retained() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"hello").unwrap();
        let cache = ReadCache::new();
        let _ = cache.read_index_path(dir.path(), std::path::Path::new("a.txt"), 0);
        let _ = cache.read_index_path(dir.path(), std::path::Path::new("a.txt"), 0);
        assert_eq!(cache.reads(), 2, "an unrelated file is not retained");
    }

    #[test]
    fn missing_and_escapes_are_their_own_outcomes() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ReadCache::new();
        assert_eq!(
            cache.read(dir.path(), &literal("nope"), 0).as_ref(),
            &ResourceRead::Missing
        );
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/etc", dir.path().join("link")).unwrap();
            let escaped = cache.read(dir.path(), &literal("link/passwd"), 0);
            assert!(
                matches!(escaped.as_ref(), ResourceRead::Unreadable(message) if message.contains("escapes"))
            );
        }
    }

    #[test]
    fn a_decode_happens_once_per_adapter_specification() {
        let cache = DecodeCache::new();
        let resource = literal("a.txt");
        let adapter = ExtractAdapter::new(Extract::WholeFile);
        let first = cache
            .decode(&adapter, &resource, b"hello", &Limits::default())
            .unwrap();
        let second = cache
            .decode(&adapter, &resource, b"hello", &Limits::default())
            .unwrap();
        assert_eq!(cache.decodes(), 1);
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(first.rows[0].value(), Some(&Value::String("hello".into())));
    }

    #[test]
    fn two_extract_specifications_decode_twice() {
        let cache = DecodeCache::new();
        let resource = literal("a.json");
        let a = ExtractAdapter::new(
            Extract::structured(crate::structured_format::Format::Json, "$.a").unwrap(),
        );
        let b = ExtractAdapter::new(
            Extract::structured(crate::structured_format::Format::Json, "$.b").unwrap(),
        );
        let limits = Limits::default();
        cache
            .decode(&a, &resource, br#"{"a":1,"b":2}"#, &limits)
            .unwrap();
        cache
            .decode(&b, &resource, br#"{"a":1,"b":2}"#, &limits)
            .unwrap();
        assert_eq!(cache.decodes(), 2);
    }

    #[test]
    fn concurrent_consumers_share_one_initializer() {
        let cache = DecodeCache::new();
        let resource = literal("a.txt");
        let adapter = ExtractAdapter::new(Extract::WholeFile);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    cache
                        .decode(&adapter, &resource, b"hello", &Limits::default())
                        .unwrap();
                });
            }
        });
        assert_eq!(cache.decodes(), 1);
    }

    #[test]
    fn limits_are_enforced_through_the_cache() {
        let cache = DecodeCache::new();
        let resource = literal("a.txt");
        let adapter = ExtractAdapter::new(Extract::Regex(r"(?m)^(.*)$".to_string()));
        let limits = Limits::default()
            .with(LimitKind::RowsPerExtractor, 1)
            .unwrap();
        assert!(matches!(
            cache.decode(&adapter, &resource, b"a\nb\n", &limits),
            Err(AdapterError::Limit(_))
        ));
    }
}
