//! The budget of YAML input that parses at once, across the process.
//!
//! A YAML parse costs far more memory than its input: at the 32 MiB input cap
//! ([`crate::MAX_STRUCTURED_BYTES`]) one parse measured 4.5 to 6.2 GB of peak
//! RSS (ADR-0042), and the per-file rules parse in parallel. So every parse of
//! repository YAML over [`YAML_BUDGET_THRESHOLD_BYTES`] first takes its input's
//! length from a process-wide budget of [`YAML_PARSE_BUDGET_BYTES`], waiting
//! until enough is free, and gives it back when the parse ends: at most one
//! maximal file parses at a time, and a file of 1 MiB or less never waits.
//!
//! The budget is held while the parser runs, not while the parsed value lives.
//! A parse's peak is inside the parser, whose events and half-built tree come
//! to several times the finished value, and the code that holds a value may
//! parse another file (a fixer re-reads the document it edits), which would
//! wait on itself if values held budget. Within a reservation nothing reserves
//! again: the parsers run no rule code and start no rayon work. A reservation
//! a thread asks for while it holds one takes nothing and never waits, so a
//! nested parse cannot deadlock either.
//!
//! The entry points that parse repository YAML take a reservation:
//! [`crate::Format::parse`], the entry rows of [`crate::extract`], the
//! structured-fix span resolver and `pinned_references`. Configuration files
//! are read one at a time and take none.

use std::cell::Cell;
use std::marker::PhantomData;
use std::sync::{Condvar, Mutex, PoisonError};

/// The YAML input, in bytes, that may be parsing at once: one file at the
/// structured-input cap.
pub const YAML_PARSE_BUDGET_BYTES: usize = crate::MAX_STRUCTURED_BYTES;

/// A YAML input of at most this many bytes parses without the budget.
pub const YAML_BUDGET_THRESHOLD_BYTES: usize = 1024 * 1024;

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

/// Reserve `input_len` bytes of the process's budget for a parse of that much
/// YAML, waiting until they are free. `None`, without waiting, for an input of
/// at most [`YAML_BUDGET_THRESHOLD_BYTES`], and on a thread that already holds
/// a reservation. An input longer than the budget takes all of it.
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

impl Drop for Reservation {
    fn drop(&mut self) {
        HELD.with(|held| held.set(false));
        self.budget.release(self.bytes);
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
    /// Threads waiting for bytes, and the most bytes ever in flight, for tests.
    #[cfg(test)]
    waiting: usize,
    #[cfg(test)]
    max_in_flight: usize,
}

impl Budget {
    pub(crate) const fn new(capacity: usize) -> Self {
        Self {
            capacity,
            state: Mutex::new(State {
                in_flight: 0,
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
        // The lock guards only counters, so a poisoned one is still sound.
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        #[cfg(test)]
        {
            state.waiting += 1;
        }
        while state.in_flight + bytes > self.capacity {
            state = self
                .freed
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
        state.in_flight += bytes;
        #[cfg(test)]
        {
            state.waiting -= 1;
            state.max_in_flight = state.max_in_flight.max(state.in_flight);
        }
        drop(state);
        HELD.with(|held| held.set(true));
        TAKEN.with(|taken| taken.set(taken.get() + 1));
        Some(Reservation {
            budget: self,
            bytes,
            _not_send: PhantomData,
        })
    }

    fn release(&self, bytes: usize) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.in_flight -= bytes;
        drop(state);
        self.freed.notify_all();
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
