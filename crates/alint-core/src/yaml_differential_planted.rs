//! Defects planted in ayaml's results, which the harness must catch: a
//! planted defect is the regression the harness exists to find, so an entry
//! that explains one explains a difference its trigger did not cause.

use std::cell::Cell;

use super::*;

/// A change to ayaml's result for an input at an entry point.
pub(super) type Plant = fn(&str, Entry, Side) -> Side;

thread_local! {
    /// The defect planted on this thread, if any; each test runs on a
    /// thread of its own.
    static PLANTED: Cell<Option<Plant>> = const { Cell::new(None) };
}

/// ayaml's result `side` for `input` at `entry`, with the planted defect.
pub(super) fn apply(input: &str, entry: Entry, side: Side) -> Side {
    match PLANTED.with(Cell::get) {
        Some(plant) => plant(input, entry, side),
        None => side,
    }
}

/// Run `body` with `plant` planted, and without it again after, even when
/// `body` panics.
pub(super) fn with_plant<T>(plant: Plant, body: impl FnOnce() -> T) -> T {
    struct Unplant;
    impl Drop for Unplant {
        fn drop(&mut self) {
            PLANTED.with(|planted| planted.set(None));
        }
    }
    PLANTED.with(|planted| planted.set(Some(plant)));
    let _unplant = Unplant;
    body()
}

/// The case for `input` at `entry` without the plant and with it.
pub(super) fn clean_and_planted(input: &str, entry: Entry, plant: Plant) -> (Case<'_>, Case<'_>) {
    let (clean, _) = compare(input, entry);
    let (planted, _) = with_plant(plant, || compare(input, entry));
    (clean, planted)
}

/// What explains `case` with `plant` planted, as the search compares every
/// rewrite with it.
pub(super) fn planted_explanation(case: &Case<'_>, plant: Plant) -> Option<Vec<&'static str>> {
    with_plant(plant, || explanation(case))
}
