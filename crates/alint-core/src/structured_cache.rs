//! Run-scoped and file-scoped caches of parsed structured documents (#548).
//!
//! Two complementary scopes avoid redundant structured parses across rules
//! without bloating memory or adding contention on per-file-only runs:
//!
//! - [`FileStructuredCache`] lives on the stack for a single file's evaluation
//!   when two or more per-file rules match that file (`dispatch_per_file` and
//!   `Engine::run_for_file`). It parses each `(file, Format)` once, shares the
//!   [`SharedValue`] across those rules with zero cross-worker contention, and
//!   drops immediately when that file finishes. Single-rule files bypass it
//!   completely.
//! - [`StructuredCache`] lives on the stack of [`crate::Engine::run`] when
//!   cross-file rules can share parsed documents with other cross-file rules
//!   or the per-file partition. It shares `Arc`-backed [`SharedValue`]s across
//!   rayon workers with per-key in-flight deduplication and LRU eviction under
//!   a byte budget (default [`crate::yaml_budget::YAML_PARSE_BUDGET_BYTES`]).
//!
//! Alias-amplified YAML trees carrying a [`crate::yaml_budget::RetainedLease`]
//! are never pinned strongly in [`StructuredCache`]'s idle LRU: they are shared
//! only via `Weak` references while actively in use so an idle cache entry can
//! never hold the YAML parse budget hostage across unrelated parses (#582).

use std::collections::{HashMap, VecDeque};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock, Weak};

use parking_lot::Mutex;
use sha2::{Digest as _, Sha256};

use crate::structured_format::{Format, SharedValue, SharedValueInner, estimate_value_bytes};

/// What a cached parse stores: the shared tree or the deterministic parse error.
type ParseOutcome = Result<SharedValue, String>;

/// Content-addressed cache key for `(repo-relative path, Format, strict_json, text digest)`.
///
/// Including `(strict_json, text_len, sha256)` guarantees that calling
/// [`crate::Context::parse_structured`] or [`crate::Context::parse_structured_strict`]
/// with different modes or text can never return a cached parse from the wrong
/// dialect or bytes (ADR-0050 D7).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    path: PathBuf,
    format: Format,
    strict_json: bool,
    text_len: usize,
    text_sha256: [u8; 32],
}

impl CacheKey {
    fn new(path: &Path, format: Format, strict_json: bool, text: &str) -> Self {
        Self {
            path: normalize_key_path(path),
            format,
            strict_json: strict_json && format == Format::Json,
            text_len: text.len(),
            text_sha256: Sha256::digest(text.as_bytes()).into(),
        }
    }
}

/// Normalize a repo-relative path to `/`-separated components (skipping
/// redundant `./` segments) so `a\b.yml`, `./a/b.yml`, and `a/b.yml` share one
/// entry while out-of-root or non-UTF-8 paths keep their OS representation.
fn normalize_key_path(relative: &Path) -> PathBuf {
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => match part.to_str() {
                Some(part) => {
                    // Normalize embedded backslashes too (e.g. Windows-style
                    // paths supplied on Unix).
                    for segment in part.split('\\') {
                        if !segment.is_empty() && segment != "." {
                            parts.push(segment);
                        }
                    }
                }
                None => return relative.to_path_buf(),
            },
            _ => return relative.to_path_buf(),
        }
    }
    PathBuf::from(parts.join("/"))
}

struct CacheEntry {
    outcome: ParseOutcome,
    bytes: usize,
    tick: u64,
}

#[derive(Default)]
struct CacheState {
    entries: HashMap<CacheKey, CacheEntry>,
    weak_leased: HashMap<CacheKey, Weak<SharedValueInner>>,
    in_flight: HashMap<CacheKey, Arc<OnceLock<ParseOutcome>>>,
    lru: VecDeque<(u64, CacheKey)>,
    used_bytes: usize,
    next_tick: u64,
}

impl CacheState {
    fn bump_tick(&mut self) -> u64 {
        self.next_tick = self.next_tick.wrapping_add(1);
        self.next_tick
    }

    fn touch(&mut self, key: &CacheKey) -> Option<ParseOutcome> {
        let tick = self.bump_tick();
        let outcome = self.entries.get_mut(key).map(|entry| {
            entry.tick = tick;
            entry.outcome.clone()
        })?;
        self.lru.push_back((tick, key.clone()));
        self.compact_lru_if_needed();
        Some(outcome)
    }

    fn compact_lru_if_needed(&mut self) {
        if self.lru.len() > self.entries.len().saturating_mul(4).saturating_add(64) {
            self.lru.retain(|(tick, key)| {
                self.entries
                    .get(key)
                    .is_some_and(|entry| entry.tick == *tick)
            });
        }
    }

    fn evict_for(&mut self, needed: usize, budget_bytes: usize, evictions: &AtomicUsize) {
        while self.used_bytes.saturating_add(needed) > budget_bytes {
            let Some((tick, key)) = self.lru.pop_front() else {
                break;
            };
            let should_evict = self
                .entries
                .get(&key)
                .is_some_and(|entry| entry.tick == tick);
            if should_evict && let Some(removed) = self.entries.remove(&key) {
                self.used_bytes = self.used_bytes.saturating_sub(removed.bytes);
                evictions.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

/// Run-scoped, memory-budgeted LRU cache of parsed structured documents shared
/// across rayon workers.
pub(crate) struct StructuredCache {
    budget_bytes: usize,
    state: Mutex<CacheState>,
    populated: AtomicBool,
    parses: AtomicUsize,
    hits: AtomicUsize,
    evictions: AtomicUsize,
}

impl std::fmt::Debug for StructuredCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print cached document values or error strings: `Context`
        // derives `Debug`, and logs must not carry repository contents.
        let state = self.state.lock();
        f.debug_struct("StructuredCache")
            .field("entries", &state.entries.len())
            .field("weak_leased", &state.weak_leased.len())
            .field("used_bytes", &state.used_bytes)
            .field("budget_bytes", &self.budget_bytes)
            .field("populated", &self.populated.load(Ordering::Relaxed))
            .field("parses", &self.parses.load(Ordering::Relaxed))
            .field("hits", &self.hits.load(Ordering::Relaxed))
            .field("evictions", &self.evictions.load(Ordering::Relaxed))
            .finish()
    }
}

struct InFlightGuard<'a> {
    cache: &'a StructuredCache,
    key: Option<CacheKey>,
    slot: Arc<OnceLock<ParseOutcome>>,
}

impl Drop for InFlightGuard<'_> {
    fn drop(&mut self) {
        if let Some(key) = self.key.take() {
            let mut state = self.cache.state.lock();
            if state
                .in_flight
                .get(&key)
                .is_some_and(|cur| Arc::ptr_eq(cur, &self.slot))
            {
                state.in_flight.remove(&key);
            }
        }
    }
}

impl StructuredCache {
    #[must_use]
    pub(crate) fn new(budget_bytes: usize) -> Self {
        Self {
            budget_bytes,
            state: Mutex::new(CacheState::default()),
            populated: AtomicBool::new(false),
            parses: AtomicUsize::new(0),
            hits: AtomicUsize::new(0),
            evictions: AtomicUsize::new(0),
        }
    }

    /// Cheap check whether any entry has been inserted in this run.
    #[must_use]
    pub(crate) fn is_populated(&self) -> bool {
        self.populated.load(Ordering::Acquire)
    }

    /// Look up an already-cached entry without parsing on a miss.
    #[cfg(test)]
    pub(crate) fn get_cached(
        &self,
        path: &Path,
        format: Format,
        text: &str,
    ) -> Option<ParseOutcome> {
        self.get_cached_with_options(path, format, false, text)
    }

    /// Look up an already-cached entry with explicit strict-JSON mode.
    pub(crate) fn get_cached_with_options(
        &self,
        path: &Path,
        format: Format,
        strict_json: bool,
        text: &str,
    ) -> Option<ParseOutcome> {
        if !self.is_populated() {
            return None;
        }
        let key = CacheKey::new(path, format, strict_json, text);
        let mut state = self.state.lock();
        if let Some(outcome) = state.touch(&key) {
            self.hits.fetch_add(1, Ordering::Relaxed);
            return Some(outcome);
        }
        if let Some(weak) = state.weak_leased.get(&key) {
            if let Some(inner) = weak.upgrade() {
                self.hits.fetch_add(1, Ordering::Relaxed);
                return Some(Ok(SharedValue::from_inner(inner)));
            }
            state.weak_leased.remove(&key);
        }
        None
    }

    /// Return the cached tree for `(path, format, text)`, parsing at most once
    /// across concurrent callers and retaining non-alias-leased results under
    /// the byte budget.
    #[cfg(test)]
    pub(crate) fn get_or_parse(&self, path: &Path, format: Format, text: &str) -> ParseOutcome {
        self.get_or_parse_with_options(path, format, false, text)
    }

    /// Return the cached tree for `(path, format, strict_json, text)`,
    /// isolating strict RFC 8259 JSON parses from tolerant JSONC parses under
    /// distinct cache keys (ADR-0050 D7).
    pub(crate) fn get_or_parse_with_options(
        &self,
        path: &Path,
        format: Format,
        strict_json: bool,
        text: &str,
    ) -> ParseOutcome {
        let strict_json = strict_json && format == Format::Json;
        if self.budget_bytes == 0 {
            self.parses.fetch_add(1, Ordering::Relaxed);
            return format.parse_shared_with_strict_json(text, strict_json);
        }
        let key = CacheKey::new(path, format, strict_json, text);
        // If this thread already holds an in-flight YAML reservation or a live
        // alias-expanded `RetainedLease`, waiting on another thread's
        // `in_flight` `OnceLock` can deadlock when that owner is itself blocked
        // on `yaml_budget` waiting for this thread's lease to drop.
        let bypass_in_flight =
            format == Format::Yaml && crate::yaml_budget::current_thread_holds_budget();
        let (slot, is_owner) = {
            let mut state = self.state.lock();
            if let Some(outcome) = state.touch(&key) {
                self.hits.fetch_add(1, Ordering::Relaxed);
                return outcome;
            }
            if let Some(weak) = state.weak_leased.get(&key) {
                if let Some(inner) = weak.upgrade() {
                    self.hits.fetch_add(1, Ordering::Relaxed);
                    return Ok(SharedValue::from_inner(inner));
                }
                state.weak_leased.remove(&key);
            }
            if bypass_in_flight {
                drop(state);
                self.parses.fetch_add(1, Ordering::Relaxed);
                let outcome = format.parse_shared_with_strict_json(text, strict_json);
                self.publish_outcome(key, None, &outcome);
                return outcome;
            }
            if let Some(existing) = state.in_flight.get(&key) {
                (Arc::clone(existing), false)
            } else {
                let slot = Arc::new(OnceLock::new());
                state.in_flight.insert(key.clone(), Arc::clone(&slot));
                (slot, true)
            }
        };

        if !is_owner {
            let mut parsed_here = false;
            let outcome = slot
                .get_or_init(|| {
                    parsed_here = true;
                    self.parses.fetch_add(1, Ordering::Relaxed);
                    format.parse_shared_with_strict_json(text, strict_json)
                })
                .clone();
            if parsed_here {
                // Non-owner won the `OnceLock` race (or took over after an
                // owner panic); publish so subsequent lookups reuse the tree.
                self.publish_outcome(key, Some(&slot), &outcome);
            } else {
                self.hits.fetch_add(1, Ordering::Relaxed);
            }
            return outcome;
        }

        let mut guard = InFlightGuard {
            cache: self,
            key: Some(key.clone()),
            slot: Arc::clone(&slot),
        };

        let mut parsed_here = false;
        let outcome = slot
            .get_or_init(|| {
                parsed_here = true;
                self.parses.fetch_add(1, Ordering::Relaxed);
                format.parse_shared_with_strict_json(text, strict_json)
            })
            .clone();

        if parsed_here {
            // Publish into LRU or weak-leased table and clear in-flight slot
            // under one lock so waiting readers transition smoothly.
            self.publish_outcome(key, Some(&slot), &outcome);
            guard.key = None;
        }
        outcome
    }

    fn publish_outcome(
        &self,
        key: CacheKey,
        slot: Option<&Arc<OnceLock<ParseOutcome>>>,
        outcome: &ParseOutcome,
    ) {
        // Estimate tree size BEFORE acquiring the global cache mutex so walking
        // a large JSON tree never stalls other rayon workers.
        let non_leased_bytes = match outcome {
            Ok(shared) if shared.has_retained_lease() => None,
            Ok(shared) => Some(estimate_value_bytes(shared)),
            Err(message) => Some(message.capacity().saturating_add(64)),
        };

        let mut state = self.state.lock();
        if let Some(expected_slot) = slot
            && state
                .in_flight
                .get(&key)
                .is_some_and(|cur| Arc::ptr_eq(cur, expected_slot))
        {
            state.in_flight.remove(&key);
        }
        match (outcome, non_leased_bytes) {
            (Ok(shared), None) => {
                // Never strongly pin an alias-leased tree in the idle LRU:
                // only share it weakly while active readers keep it alive.
                state.weak_leased.retain(|_, weak| weak.strong_count() > 0);
                state.weak_leased.insert(key, shared.downgrade());
                self.populated.store(true, Ordering::Release);
            }
            (_, Some(entry_bytes)) => {
                self.insert_lru_locked(&mut state, key, outcome, entry_bytes);
            }
            (Err(_), None) => {}
        }
    }

    fn insert_lru_locked(
        &self,
        state: &mut CacheState,
        key: CacheKey,
        outcome: &ParseOutcome,
        entry_bytes: usize,
    ) {
        if entry_bytes > self.budget_bytes {
            return;
        }
        if let Some(prev) = state.entries.remove(&key) {
            state.used_bytes = state.used_bytes.saturating_sub(prev.bytes);
        }
        state.evict_for(entry_bytes, self.budget_bytes, &self.evictions);
        let tick = state.bump_tick();
        state.used_bytes = state.used_bytes.saturating_add(entry_bytes);
        state.entries.insert(
            key.clone(),
            CacheEntry {
                outcome: outcome.clone(),
                bytes: entry_bytes,
                tick,
            },
        );
        state.lru.push_back((tick, key));
        state.compact_lru_if_needed();
        self.populated.store(true, Ordering::Release);
    }

    #[cfg(test)]
    pub(crate) fn parses(&self) -> usize {
        self.parses.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    pub(crate) fn hits(&self) -> usize {
        self.hits.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    pub(crate) fn evictions(&self) -> usize {
        self.evictions.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    pub(crate) fn used_bytes(&self) -> usize {
        self.state.lock().used_bytes
    }
}

/// Stack-local cache for a single file being evaluated by 2+ per-file rules.
///
/// Uncontended (owned by the thread running `dispatch_per_file` /
/// `Engine::run_for_file`) and dropped immediately once that file's rules
/// finish, keeping peak memory proportional to worker count rather than file
/// count. Keyed by `(path, format, text digest)` so companion or modified reads
/// never collide, and never strongly pins alias-leased `SharedValue`s across
/// subsequent rules (preventing hold-and-wait against non-structured rules such
/// as `pinned_references`).
pub(crate) struct FileStructuredCache<'a> {
    run: Option<&'a StructuredCache>,
    slots: Mutex<Vec<(CacheKey, ParseOutcome)>>,
}

impl std::fmt::Debug for FileStructuredCache<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileStructuredCache")
            .field("entries", &self.slots.lock().len())
            .field("has_run_cache", &self.run.is_some())
            .finish()
    }
}

impl<'a> FileStructuredCache<'a> {
    #[must_use]
    pub(crate) fn new(run: Option<&'a StructuredCache>) -> Self {
        Self {
            run,
            slots: Mutex::new(Vec::new()),
        }
    }

    #[cfg(test)]
    pub(crate) fn get_or_parse(&self, path: &Path, format: Format, text: &str) -> ParseOutcome {
        self.get_or_parse_with_options(path, format, false, text)
    }

    pub(crate) fn get_or_parse_with_options(
        &self,
        path: &Path,
        format: Format,
        strict_json: bool,
        text: &str,
    ) -> ParseOutcome {
        let strict_json = strict_json && format == Format::Json;
        let key = CacheKey::new(path, format, strict_json, text);
        {
            let slots = self.slots.lock();
            if let Some((_, outcome)) = slots.iter().find(|(k, _)| *k == key) {
                return outcome.clone();
            }
        }
        let outcome = if let Some(hit) = self
            .run
            .and_then(|run| run.get_cached_with_options(path, format, strict_json, text))
        {
            hit
        } else {
            format.parse_shared_with_strict_json(text, strict_json)
        };
        // Never strongly pin an alias-leased tree across unrelated per-file
        // rules (e.g., `yaml_path_*` followed by `pinned_references`).
        if matches!(&outcome, Ok(shared) if shared.has_retained_lease()) {
            return outcome;
        }
        let mut slots = self.slots.lock();
        if let Some((_, existing)) = slots.iter().find(|(k, _)| *k == key) {
            return existing.clone();
        }
        slots.push((key, outcome.clone()));
        outcome
    }
}

/// Borrowed reference stored inside [`crate::Context`].
#[derive(Debug, Clone, Copy)]
pub(crate) enum StructuredCacheRef<'a> {
    Run(&'a StructuredCache),
    /// Read-only view of the run cache used by single-rule per-file dispatch:
    /// reuses trees already parsed by cross-file rules without retaining new
    /// trees after the per-file pass finishes.
    RunReadOnly(&'a StructuredCache),
    File(&'a FileStructuredCache<'a>),
}

impl<'a> StructuredCacheRef<'a> {
    pub(crate) fn run_cache(self) -> Option<&'a StructuredCache> {
        match self {
            Self::Run(run) | Self::RunReadOnly(run) => Some(run),
            Self::File(file) => file.run,
        }
    }

    pub(crate) fn get_or_parse_with_options(
        self,
        path: &Path,
        format: Format,
        strict_json: bool,
        text: &str,
    ) -> ParseOutcome {
        let strict_json = strict_json && format == Format::Json;
        match self {
            Self::Run(run) => run.get_or_parse_with_options(path, format, strict_json, text),
            Self::RunReadOnly(run) => {
                if let Some(hit) = run.get_cached_with_options(path, format, strict_json, text) {
                    hit
                } else {
                    format.parse_shared_with_strict_json(text, strict_json)
                }
            }
            Self::File(file) => file.get_or_parse_with_options(path, format, strict_json, text),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Write as _;
    use std::sync::Barrier;

    #[test]
    fn run_cache_shares_trees_per_path_and_format() {
        let cache = StructuredCache::new(1024 * 1024);
        let path = Path::new("config/app.yml");
        let text = "name: alint\nport: 8080\n";

        let first = cache.get_or_parse(path, Format::Yaml, text).unwrap();
        let second = cache.get_or_parse(path, Format::Yaml, text).unwrap();
        assert_eq!(*first, *second);
        assert!(first.ptr_eq(&second));
        assert_eq!(cache.parses(), 1);
        assert_eq!(cache.hits(), 1);

        // Redundant `./` and Windows-style `\` separators normalize to the same key.
        let dot_alt = cache
            .get_or_parse(Path::new("./config/app.yml"), Format::Yaml, text)
            .unwrap();
        assert!(first.ptr_eq(&dot_alt));
        let slash_alt = cache
            .get_or_parse(Path::new(r"config\app.yml"), Format::Yaml, text)
            .unwrap();
        assert!(first.ptr_eq(&slash_alt));
        assert_eq!(cache.parses(), 1);
        assert_eq!(cache.hits(), 3);

        // Different text under the same `(path, format)` does not collide.
        let different = cache
            .get_or_parse(path, Format::Yaml, "name: other\nport: 9090\n")
            .unwrap();
        assert!(!first.ptr_eq(&different));
        assert_eq!(different["name"], "other");
        assert_eq!(cache.parses(), 2);

        // Distinct format on the same path parses separately.
        let json_res = cache.get_or_parse(path, Format::Json, "{\"name\": \"alint\"}");
        assert!(json_res.is_ok());
        assert_eq!(cache.parses(), 3);
    }

    #[test]
    fn run_cache_evicts_lru_entries_to_stay_within_byte_budget() {
        let payload = format!("data: \"{}\"\n", "a".repeat(2048));
        let sample = Format::Yaml.parse_shared(&payload).unwrap();
        let entry_size = estimate_value_bytes(&sample);
        // Budget fits two entries, not three.
        let budget = entry_size * 2 + 128;
        let cache = StructuredCache::new(budget);

        let a = Path::new("a.yml");
        let b = Path::new("b.yml");
        let c = Path::new("c.yml");

        let _ = cache.get_or_parse(a, Format::Yaml, &payload).unwrap();
        let _ = cache.get_or_parse(b, Format::Yaml, &payload).unwrap();
        assert!(cache.used_bytes() <= budget);
        // Touch `a` so `b` becomes least recently used.
        let _ = cache.get_or_parse(a, Format::Yaml, &payload).unwrap();
        assert_eq!(cache.hits(), 1);

        let _ = cache.get_or_parse(c, Format::Yaml, &payload).unwrap();
        assert!(cache.used_bytes() <= budget);
        assert_eq!(cache.evictions(), 1);

        // `a` is still cached; `b` was evicted.
        assert!(cache.get_cached(a, Format::Yaml, &payload).is_some());
        assert!(cache.get_cached(b, Format::Yaml, &payload).is_none());
        assert!(cache.get_cached(c, Format::Yaml, &payload).is_some());
    }

    #[test]
    fn oversized_entry_is_returned_without_wedging_cache() {
        let small = "k: 1\n";
        let big = format!("k: \"{}\"\n", "x".repeat(4096));
        let small_size = estimate_value_bytes(&Format::Yaml.parse_shared(small).unwrap());
        let cache = StructuredCache::new(small_size + 256);

        let _ = cache
            .get_or_parse(Path::new("small.yml"), Format::Yaml, small)
            .unwrap();
        let big_val = cache
            .get_or_parse(Path::new("big.yml"), Format::Yaml, &big)
            .unwrap();
        assert_eq!(big_val["k"].as_str().unwrap().len(), 4096);
        assert!(
            cache
                .get_cached(Path::new("big.yml"), Format::Yaml, &big)
                .is_none()
        );
        assert!(
            cache
                .get_cached(Path::new("small.yml"), Format::Yaml, small)
                .is_some()
        );
    }

    #[test]
    fn concurrent_callers_parse_same_file_once() {
        let cache = Arc::new(StructuredCache::new(4 * 1024 * 1024));
        let mut raw = String::new();
        for i in 0..200 {
            let _ = writeln!(raw, "key{i}: value{i}");
        }
        let text = Arc::new(raw);
        let barrier = Arc::new(Barrier::new(8));
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let cache = Arc::clone(&cache);
                let text = Arc::clone(&text);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    cache
                        .get_or_parse(Path::new("shared.yml"), Format::Yaml, &text)
                        .unwrap()
                })
            })
            .collect();
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        for item in &results[1..] {
            assert!(results[0].ptr_eq(item));
        }
        assert_eq!(cache.parses(), 1);
    }

    #[test]
    fn file_cache_reuses_run_cache_and_shares_without_populating_run_cache() {
        let run = StructuredCache::new(1024 * 1024);
        let file_cache = FileStructuredCache::new(Some(&run));
        let p = Path::new("svc.yml");
        let text = "openapi: 3.0.0\n";

        let a = file_cache.get_or_parse(p, Format::Yaml, text).unwrap();
        let b = file_cache.get_or_parse(p, Format::Yaml, text).unwrap();
        assert!(a.ptr_eq(&b));
        // Distinct path or distinct text in the same `FileStructuredCache` must
        // not collide with the primary file's entry.
        let companion = file_cache
            .get_or_parse(Path::new("other.yml"), Format::Yaml, "openapi: 3.1.0\n")
            .unwrap();
        assert!(!a.ptr_eq(&companion));
        assert_eq!(companion["openapi"], "3.1.0");
        let modified = file_cache
            .get_or_parse(p, Format::Yaml, "openapi: 3.2.0\n")
            .unwrap();
        assert!(!a.ptr_eq(&modified));
        assert_eq!(modified["openapi"], "3.2.0");
        // Per-file-only parses do not retain entries in the run cache.
        assert!(!run.is_populated());

        // When the run cache already holds the tree, the file cache reuses it.
        let p2 = Path::new("shared.yml");
        let from_run = run.get_or_parse(p2, Format::Yaml, text).unwrap();
        let file_cache2 = FileStructuredCache::new(Some(&run));
        let from_file = file_cache2.get_or_parse(p2, Format::Yaml, text).unwrap();
        assert!(from_run.ptr_eq(&from_file));
        assert_eq!(run.parses(), 1);
    }

    #[test]
    fn alias_leased_tree_shares_while_live_and_does_not_pin_idle_budget() {
        let cache = StructuredCache::new(64 * 1024 * 1024);
        let mut text = format!("a: &a \"{}\"\nb:\n", "x".repeat(140_000));
        for _ in 0..500 {
            text.push_str("  - *a\n");
        }
        let path = Path::new("alias.yml");
        let first = cache.get_or_parse(path, Format::Yaml, &text).unwrap();
        assert!(first.has_retained_lease());
        // While live, a second lookup shares the exact same `Arc` via weak ref.
        let second = cache.get_or_parse(path, Format::Yaml, &text).unwrap();
        assert!(first.ptr_eq(&second));
        assert_eq!(cache.parses(), 1);
        assert_eq!(
            cache.used_bytes(),
            0,
            "alias-leased tree is not pinned in LRU"
        );

        // `FileStructuredCache` also refuses to strongly pin alias-leased trees
        // across subsequent rules.
        let file_cache = FileStructuredCache::new(None);
        let from_file = file_cache.get_or_parse(path, Format::Yaml, &text).unwrap();
        assert!(from_file.has_retained_lease());
        assert_eq!(file_cache.slots.lock().len(), 0);
        drop(from_file);

        // Once every caller drops it, the weak entry expires and nothing holds
        // the `RetainedLease` idle in the cache.
        drop(first);
        drop(second);
        assert!(cache.get_cached(path, Format::Yaml, &text).is_none());
    }

    #[test]
    fn strict_and_tolerant_json_cache_entries_are_isolated() {
        let jsonc = "{\n  // comment\n  \"a\": 1,\n}\n";
        let path_a = Path::new("tsconfig.json");
        let path_b = Path::new("settings.json");

        // Tolerant first, then strict on the same StructuredCache: strict must
        // not reuse the tolerant JSONC tree.
        let run_cache = StructuredCache::new(8 * 1024 * 1024);
        let tolerant = run_cache
            .get_or_parse_with_options(path_a, Format::Json, false, jsonc)
            .expect("tolerant JSONC parses");
        assert_eq!(tolerant["a"], 1);
        let strict_err = run_cache
            .get_or_parse_with_options(path_a, Format::Json, true, jsonc)
            .expect_err("strict JSON must reject JSONC even after tolerant hit");
        assert_ne!(strict_err, "");
        assert_eq!(run_cache.parses(), 2);

        // Strict first, then tolerant on another path: tolerant must succeed
        // even after a strict parse failure on the same bytes.
        assert!(
            run_cache
                .get_or_parse_with_options(path_b, Format::Json, true, jsonc)
                .is_err()
        );
        let tolerant_b = run_cache
            .get_or_parse_with_options(path_b, Format::Json, false, jsonc)
            .expect("tolerant JSONC succeeds after strict miss");
        assert_eq!(tolerant_b["a"], 1);

        // FileStructuredCache must enforce the same isolation.
        let file_cache = FileStructuredCache::new(None);
        let f_tol = file_cache
            .get_or_parse_with_options(path_a, Format::Json, false, jsonc)
            .expect("file cache tolerant succeeds");
        assert_eq!(f_tol["a"], 1);
        assert!(
            file_cache
                .get_or_parse_with_options(path_a, Format::Json, true, jsonc)
                .is_err(),
            "file cache strict must not reuse tolerant entry"
        );
    }

    #[test]
    fn yaml_retained_lease_never_deadlocks_against_in_flight_singleflight() {
        // Regression for audit deadlock: a worker already holding a YAML
        // `RetainedLease` calling `get_or_parse` on another YAML file must
        // bypass `state.in_flight` wait instead of blocking on another worker
        // that is waiting on `yaml_budget::reserve`.
        let cache = Arc::new(StructuredCache::new(64 * 1024));
        let path_a = Path::new("a.yaml");
        let path_b = Path::new("b.yaml");
        let doc_a = "base: &a [1, 2, 3, 4]\nx: [*a, *a, *a, *a, *a, *a]\n";
        let doc_b = "base: &b [5, 6, 7, 8]\ny: [*b, *b, *b, *b, *b, *b]\n";

        let lease_a = cache
            .get_or_parse(path_a, Format::Yaml, doc_a)
            .expect("first alias YAML parses");
        let cache2 = Arc::clone(&cache);
        let handle = std::thread::spawn(move || {
            let v = cache2
                .get_or_parse(path_b, Format::Yaml, doc_b)
                .expect("second alias YAML parses without deadlock");
            assert!(v["y"].is_array());
        });
        let v_main = cache
            .get_or_parse(path_b, Format::Yaml, doc_b)
            .expect("main thread holding lease parses without deadlock");
        assert!(v_main["y"].is_array());
        drop(lease_a);
        handle.join().expect("worker thread completes");
    }
}
