//! The budget of YAML input that parses at once, across the process.
//!
//! A YAML parse costs far more memory than its input: at the 32 MiB input cap
//! ([`crate::MAX_STRUCTURED_BYTES`]) one parse measured 0.75 to 1.54 GiB of
//! peak RSS with ayaml 0.2.2 (down from 4.5 to 6.2 GB with ayaml 0.1.2,
//! ADR-0042), and the per-file rules parse in parallel. So every parse of
//! repository YAML whose cost is over [`YAML_BUDGET_THRESHOLD_BYTES`] first
//! takes that cost from a process-wide budget of [`YAML_PARSE_BUDGET_BYTES`],
//! waiting until enough is free, and gives it back when the parse ends: at
//! most one maximal file parses at a time.
//!
//! A parse's cost is its input's length, but where the parse copies what
//! aliases name into the value it builds ([`crate::Format::parse`]): there it
//! is the larger of the length and the alias guard's estimate of the expanded
//! tree, scaled to input bytes ([`parse_cost`]). A file of 500 KB with one
//! anchored string of 500 KB and 500 aliases of it peaked at 269 MB, and
//! sixteen of them at 3.1 GB, when the budget charged the length alone. A file
//! of 1 MiB or less never waits unless the guard estimates its expanded tree
//! at more than 32 MiB.
//!
//! The budget is held while the parser runs, and -- for an alias-amplified
//! document whose finished tree is roughly as large as its parse peak --
//! retained alongside the returned [`crate::SharedValue`] until the last handle
//! drops (`RetainedLease`, asamarts/alint#582). For alias-free documents (or
//! documents whose alias expansion stays within the threshold or their own
//! length), a parse peaks far above the value it leaves, so the budget is
//! returned as soon as parsing ends (ADR-0042 decision 4). Within a parse
//! reservation nothing reserves again: the parsers run no rule code and start
//! no rayon work. A reservation a thread asks for while it already holds an
//! in-flight reservation takes nothing and never waits, and if a thread is the
//! sole holder of retained budget on the process and asks to parse again (for
//! example, a fixer re-reading a document), it likewise proceeds without
//! waiting on itself so single-thread re-entry cannot deadlock.
//!
//! The entry points that parse repository YAML take a reservation:
//! [`crate::Format::parse`] / [`crate::Format::parse_shared`], the entry rows
//! of [`crate::extract`], the structured-fix span resolver and
//! `pinned_references`. Only the first copies what an alias names; the others
//! keep an alias as one node, so their cost is their input's length.
//! Configuration files are read one at a time and take none.

use std::cell::Cell;
use std::marker::PhantomData;
use std::sync::{Condvar, Mutex, PoisonError};
use std::thread::ThreadId;

/// The YAML input, in bytes, that may be parsing at once: one file at the
/// structured-input cap.
pub const YAML_PARSE_BUDGET_BYTES: usize = crate::MAX_STRUCTURED_BYTES;

/// A YAML parse that costs at most this many bytes runs without the budget.
pub const YAML_BUDGET_THRESHOLD_BYTES: usize = 1024 * 1024;

/// The bytes of the alias guard's estimate of an expanded tree that cost as
/// much as one byte of input: about what a byte of input costs a parse at its
/// peak (ayaml 0.2.2's peak heap is about 22 bytes per input byte on
/// OpenAPI-shaped YAML, rounded to a power of two; ADR-0042, ADR-0046
/// decision 7). The estimate of a tree that copies what aliases name is close
/// to the parse's peak, as the copies are most of it, so the trees of the
/// parses in flight stay within this many times the budget, 1 GiB. The
/// estimate of a document that holds an alias but replays little is far below
/// its parse's peak (about 11 bytes per input byte for OpenAPI-shaped YAML),
/// so its cost stays its length.
pub const EXPANSION_BYTES_PER_COST_BYTE: usize = 32;

/// The process's budget.
static GLOBAL: Budget = Budget::new(YAML_PARSE_BUDGET_BYTES);

std::thread_local! {
    /// Whether this thread holds a reservation.
    static HELD: Cell<bool> = const { Cell::new(false) };
    /// How many reservations this thread has taken.
    static TAKEN: Cell<usize> = const { Cell::new(0) };
    /// The budget [`reserve`] takes from on this thread, in place of the
    /// process's: a test's own (see [`with_budget`]).
    #[cfg(test)]
    static TEST_BUDGET: Cell<Option<&'static Budget>> = const { Cell::new(None) };
}

/// Reserve `input_len` bytes of the process's budget for a parse that costs
/// that much (see [`parse_cost`]), waiting until they are free. `None`,
/// without waiting, for a cost of at most [`YAML_BUDGET_THRESHOLD_BYTES`], and
/// on a thread that already holds a reservation. A cost past the budget takes
/// all of it.
///
/// Hold the result until the parser returns, and drop it on the thread that
/// took it.
#[must_use = "the reservation is given back when it is dropped"]
pub fn reserve(input_len: usize) -> Option<Reservation> {
    #[cfg(test)]
    if let Some(budget) = TEST_BUDGET.with(Cell::get) {
        return budget.reserve(input_len);
    }
    GLOBAL.reserve(input_len)
}

/// What a parse of `input_len` bytes of YAML costs when it copies what the
/// aliases name into its value: the input's length, or the alias guard's
/// estimate of the expanded tree (`expanded`, see
/// [`crate::yaml_depth::GuardScan::expansion_and_size`]) over
/// [`EXPANSION_BYTES_PER_COST_BYTE`] when that is more.
#[must_use]
pub const fn parse_cost(input_len: usize, expanded: usize) -> usize {
    let expansion = expanded / EXPANSION_BYTES_PER_COST_BYTE;
    if expansion > input_len {
        expansion
    } else {
        input_len
    }
}

/// Bytes of the YAML parse budget an alias-expanded tree retains while alive:
/// `0` when the alias-replayed bytes stay at or below
/// `YAML_BUDGET_THRESHOLD_BYTES * EXPANSION_BYTES_PER_COST_BYTE` (32 MiB);
/// otherwise `alias_replayed / EXPANSION_BYTES_PER_COST_BYTE` (#582).
///
/// Basing the retained cost on alias-replayed bytes (rather than comparing
/// total expansion against `input_len`) bounds alias-amplified trees across all
/// file sizes up to the 32 MiB input cap while leaving alias-light documents
/// uncharged.
#[must_use]
pub const fn retained_alias_cost(alias_replayed: usize) -> usize {
    let cost = alias_replayed / EXPANSION_BYTES_PER_COST_BYTE;
    if cost > YAML_BUDGET_THRESHOLD_BYTES {
        cost
    } else {
        0
    }
}

/// Charge `retained_bytes` directly to the active budget as a [`RetainedLease`]
/// when a re-entrant parse ran without an in-flight [`Reservation`], ensuring
/// re-entrant alias-expanded trees never escape the budget or enter idle LRU
/// caches (#582).
#[must_use = "the retained budget is given back when dropped"]
pub(crate) fn retain_unreserved(retained_bytes: usize) -> Option<RetainedLease> {
    if retained_bytes == 0 {
        return None;
    }
    #[cfg(test)]
    if let Some(budget) = TEST_BUDGET.with(Cell::get) {
        return budget.retain_unreserved(retained_bytes);
    }
    GLOBAL.retain_unreserved(retained_bytes)
}

/// `held`, when it holds at least `bytes` (or all of the budget), and
/// otherwise a reservation of `bytes` in its place, waiting until they are
/// free: for a parse that learns it costs more than it reserved, once the
/// alias guard has measured its input. `held` is given back first, so the
/// thread waits holding nothing. `None` under the conditions of [`reserve`].
#[must_use = "the reservation is given back when it is dropped"]
pub fn reserve_at_least(held: Option<Reservation>, bytes: usize) -> Option<Reservation> {
    if held
        .as_ref()
        .is_some_and(|held| held.bytes >= bytes.min(held.budget.capacity))
    {
        return held;
    }
    drop(held);
    reserve(bytes)
}

/// How many reservations this thread has taken, for tests that check a parse
/// takes one.
#[must_use]
pub fn reservations_on_this_thread() -> usize {
    TAKEN.with(Cell::get)
}

/// Bytes of the budget a parse holds. It gives them back when dropped, on a
/// panic as on an error.
#[must_use = "the reservation is given back when it is dropped"]
pub struct Reservation {
    budget: &'static Budget,
    bytes: usize,
    released: bool,
    /// Dropped on the thread that took it, which [`HELD`] describes.
    _not_send: PhantomData<*const ()>,
}

impl std::fmt::Debug for Reservation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reservation")
            .field("bytes", &self.bytes)
            .finish_non_exhaustive()
    }
}

impl Reservation {
    /// Transition this in-flight parse reservation atomically into a
    /// `Send + Sync` [`RetainedLease`] holding `retained_bytes` (clamped to
    /// the reservation's own bytes) without transiently freeing the budget.
    /// Clears this thread's in-flight `HELD` flag. Returns `None` (releasing
    /// the whole reservation) when `retained_bytes == 0`.
    #[must_use = "the retained budget is given back when dropped"]
    pub fn into_retained(mut self, retained_bytes: usize) -> Option<RetainedLease> {
        let keep = retained_bytes.min(self.bytes);
        if keep == 0 {
            return None;
        }
        self.budget.transition_to_retained(self.bytes, keep);
        self.released = true;
        HELD.with(|held| held.set(false));
        Some(RetainedLease {
            budget: self.budget,
            bytes: keep,
        })
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if !self.released {
            HELD.with(|held| held.set(false));
            self.budget.release(self.bytes);
        }
    }
}

/// Bytes of the YAML parse budget retained by a live alias-expanded value tree
/// after parsing ends (#582). Unlike [`Reservation`], this type is
/// `Send + Sync` so a shared `Arc` tree can cross threads and release its
/// budget when the last handle drops.
#[must_use = "the retained budget is given back when dropped"]
pub struct RetainedLease {
    budget: &'static Budget,
    bytes: usize,
}

impl RetainedLease {
    /// Register a per-handle affinity token on the calling thread so re-entrant
    /// parses on a thread holding a live `SharedValue` handle do not deadlock,
    /// while dropping that thread's handle immediately clears its affinity even
    /// if other threads keep cloned handles alive.
    pub(crate) fn register_holder(&self) -> LeaseHolder {
        let origin = std::thread::current().id();
        self.budget.add_holder(origin);
        LeaseHolder {
            budget: self.budget,
            origin,
        }
    }
}

impl std::fmt::Debug for RetainedLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RetainedLease")
            .field("bytes", &self.bytes)
            .finish_non_exhaustive()
    }
}

impl Drop for RetainedLease {
    fn drop(&mut self) {
        self.budget.release_retained(self.bytes);
    }
}

/// Per-`SharedValue` handle token tracking that `origin` holds a live
/// alias-leased tree handle. Dropping this token decrements `origin`'s count
/// regardless of which thread drops it, preventing leaks and stale bypasses
/// after cross-thread sharing.
pub(crate) struct LeaseHolder {
    budget: &'static Budget,
    origin: ThreadId,
}

impl Drop for LeaseHolder {
    fn drop(&mut self) {
        self.budget.remove_holder(self.origin);
    }
}

/// A byte semaphore: `Mutex` + `Condvar`.
pub(crate) struct Budget {
    capacity: usize,
    state: Mutex<State>,
    freed: Condvar,
}

#[derive(Default)]
struct State {
    in_flight: usize,
    retained: usize,
    /// Per-thread count of live `SharedValue` handles referencing a retained
    /// lease, used to avoid self-deadlock and multi-thread AB-BA cycles when a
    /// thread holding a retained lease initiates another budgeted parse.
    holders_by_thread: Vec<(ThreadId, usize)>,
    /// Threads waiting for bytes, and the most bytes ever held (`in_flight + retained`), for tests.
    #[cfg(test)]
    waiting: usize,
    #[cfg(test)]
    max_in_flight: usize,
}

impl State {
    const fn total(&self) -> usize {
        self.in_flight.saturating_add(self.retained)
    }

    fn holders_of(&self, tid: ThreadId) -> usize {
        self.holders_by_thread
            .iter()
            .find(|(id, _)| *id == tid)
            .map_or(0, |(_, count)| *count)
    }

    fn add_holder(&mut self, tid: ThreadId) {
        if let Some((_, count)) = self.holders_by_thread.iter_mut().find(|(id, _)| *id == tid) {
            *count += 1;
        } else {
            self.holders_by_thread.push((tid, 1));
        }
    }

    fn remove_holder(&mut self, tid: ThreadId) {
        if let Some(pos) = self.holders_by_thread.iter().position(|(id, _)| *id == tid) {
            let count = &mut self.holders_by_thread[pos].1;
            *count -= 1;
            if *count == 0 {
                self.holders_by_thread.swap_remove(pos);
            }
        }
    }
}

impl Budget {
    pub(crate) const fn new(capacity: usize) -> Self {
        Self {
            capacity,
            state: Mutex::new(State {
                in_flight: 0,
                retained: 0,
                holders_by_thread: Vec::new(),
                #[cfg(test)]
                waiting: 0,
                #[cfg(test)]
                max_in_flight: 0,
            }),
            freed: Condvar::new(),
        }
    }

    fn reserve(&'static self, input_len: usize) -> Option<Reservation> {
        if input_len <= YAML_BUDGET_THRESHOLD_BYTES || HELD.with(Cell::get) {
            return None;
        }
        let bytes = input_len.min(self.capacity);
        let current_thread = std::thread::current().id();
        // The lock guards only counters, so a poisoned one is still sound.
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        #[cfg(test)]
        {
            state.waiting += 1;
        }
        while state.total().saturating_add(bytes) > self.capacity {
            if state.holders_of(current_thread) > 0 {
                // This thread already holds at least one live alias-expanded
                // `SharedValue` handle. Blocking here can deadlock either on
                // itself (single holder) or in a multi-thread AB-BA cycle
                // across partial holders. Proceed without waiting; any new
                // alias-expanded tree still charges its retained lease via
                // `retain_unreserved`.
                #[cfg(test)]
                {
                    state.waiting -= 1;
                }
                return None;
            }
            state = self
                .freed
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
        state.in_flight += bytes;
        #[cfg(test)]
        {
            state.waiting -= 1;
            state.max_in_flight = state.max_in_flight.max(state.total());
        }
        drop(state);
        HELD.with(|held| held.set(true));
        TAKEN.with(|taken| taken.set(taken.get() + 1));
        Some(Reservation {
            budget: self,
            bytes,
            released: false,
            _not_send: PhantomData,
        })
    }

    fn release(&self, bytes: usize) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.in_flight -= bytes;
        drop(state);
        self.freed.notify_all();
    }

    fn transition_to_retained(&self, in_flight_bytes: usize, keep_bytes: usize) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.in_flight -= in_flight_bytes;
        state.retained = state.retained.saturating_add(keep_bytes);
        let freed_any = keep_bytes < in_flight_bytes;
        drop(state);
        if freed_any {
            self.freed.notify_all();
        }
    }

    fn retain_unreserved(&'static self, retained_bytes: usize) -> Option<RetainedLease> {
        let keep = retained_bytes.min(self.capacity);
        if keep == 0 {
            return None;
        }
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.retained = state.retained.saturating_add(keep);
        #[cfg(test)]
        {
            state.max_in_flight = state.max_in_flight.max(state.total());
        }
        drop(state);
        Some(RetainedLease {
            budget: self,
            bytes: keep,
        })
    }

    fn release_retained(&self, bytes: usize) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.retained = state.retained.saturating_sub(bytes);
        drop(state);
        self.freed.notify_all();
    }

    fn add_holder(&self, origin: ThreadId) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.add_holder(origin);
    }

    fn remove_holder(&self, origin: ThreadId) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.remove_holder(origin);
    }
}

/// Run `f` with [`reserve`] taking from `budget` on this thread: the test
/// seam for a budget smaller than the process's.
#[cfg(test)]
pub(crate) fn with_budget<R>(budget: &'static Budget, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<&'static Budget>);
    impl Drop for Restore {
        fn drop(&mut self) {
            TEST_BUDGET.with(|slot| slot.set(self.0));
        }
    }
    let _restore = Restore(TEST_BUDGET.with(|slot| slot.replace(Some(budget))));
    f()
}

#[cfg(test)]
#[path = "yaml_budget_tests.rs"]
mod tests;
