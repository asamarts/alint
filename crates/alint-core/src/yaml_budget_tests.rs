use super::*;
use crate::structured_fix::{PathSeg, resolve_value_span};
use crate::{Format, extract_entry_rows};
use std::fmt::Write as _;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const MIB: usize = 1024 * 1024;

/// A budget of `capacity` bytes that lives as long as the threads using it.
fn budget(capacity: usize) -> &'static Budget {
    Box::leak(Box::new(Budget::new(capacity)))
}

/// `(in flight, waiting, most ever in flight)`.
fn counters(budget: &Budget) -> (usize, usize, usize) {
    let state = budget.state.lock().unwrap();
    (state.in_flight, state.waiting, state.max_in_flight)
}

/// A YAML mapping of exactly `len` bytes: `key<i>: value <i>` lines, then a
/// comment that pads it.
fn yaml_of_len(len: usize) -> String {
    let mut text = String::with_capacity(len);
    let mut i = 0;
    loop {
        let line = format!("key{i}: value {i}\n");
        if text.len() + line.len() + 2 > len {
            break;
        }
        text.push_str(&line);
        i += 1;
    }
    let pad = len - text.len() - 2;
    let _ = writeln!(text, "#{}", "x".repeat(pad));
    assert_eq!(text.len(), len);
    text
}

/// A YAML mapping whose `b` holds `copies` aliases of one anchored string of
/// `len` bytes.
fn aliases_of(len: usize, copies: usize) -> String {
    let mut text = format!("a: &a \"{}\"\nb:\n", "x".repeat(len));
    for _ in 0..copies {
        text.push_str("  - *a\n");
    }
    text
}

/// Wait until `condition` holds, failing after a minute: a thread that should
/// not wait fails the test instead of hanging it.
fn wait_until(what: &str, condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting until {what}");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// `f` on a new thread whose reservations take from `budget`; its result once
/// it ends, which must be within a minute.
fn on_thread<R: Send + 'static>(
    budget: &'static Budget,
    f: impl FnOnce() -> R + Send + 'static,
) -> R {
    let thread: JoinHandle<R> = std::thread::spawn(move || with_budget(budget, f));
    wait_until("the thread ends", || thread.is_finished());
    thread.join().unwrap()
}

#[test]
fn two_large_parses_run_one_after_the_other() {
    let budget = budget(2 * MIB);
    let text: &'static str = Box::leak(yaml_of_len(2 * MIB).into_boxed_str());
    // A parse in flight: this thread holds the whole budget.
    let held = with_budget(budget, || reserve(text.len())).unwrap();
    let parses: Vec<_> = (0..2)
        .map(|_| {
            std::thread::spawn(move || {
                with_budget(budget, || {
                    let key7 = Format::Yaml.parse(text).map(|value| value["key7"].clone());
                    (key7, reservations_on_this_thread())
                })
            })
        })
        .collect();
    // Both wait for it, and neither has parsed.
    wait_until("both parses wait", || counters(budget).1 == 2);
    assert!(parses.iter().all(|parse| !parse.is_finished()));
    drop(held);
    wait_until("both parses end", || {
        parses.iter().all(JoinHandle::is_finished)
    });
    for parse in parses {
        let (key7, taken) = parse.join().unwrap();
        assert_eq!(key7, Ok(serde_json::json!("value 7")));
        assert_eq!(taken, 1);
    }
    // Each took the whole budget, so they ran one after the other: never more
    // than 2 MiB in flight.
    assert_eq!(counters(budget), (0, 0, 2 * MIB));
}

#[test]
fn small_parses_never_touch_the_budget() {
    let budget = budget(2 * MIB);
    // The whole budget is held, so a parse that took any would wait forever.
    let held = with_budget(budget, || reserve(2 * MIB)).unwrap();
    let small = yaml_of_len(YAML_BUDGET_THRESHOLD_BYTES);
    // Aliases whose expanded tree is a few times the file's length.
    let anchored = aliases_of(1_000, 100);
    let taken = on_thread(budget, move || {
        assert!(reserve(small.len()).is_none());
        assert!(Format::Yaml.parse(&small).is_ok());
        assert!(Format::Yaml.parse(&anchored).is_ok());
        assert!(extract_entry_rows(Format::Yaml, &small).is_ok());
        let key = [PathSeg::Key("key3".into())];
        assert!(resolve_value_span(Format::Yaml, small.as_bytes(), &key).is_some());
        reservations_on_this_thread()
    });
    assert_eq!(taken, 0);
    assert_eq!(counters(budget), (2 * MIB, 0, 2 * MIB));
    drop(held);
}

#[test]
fn each_entry_point_reserves_a_large_input() {
    let budget = budget(4 * MIB);
    let large = yaml_of_len(YAML_BUDGET_THRESHOLD_BYTES + 1);
    let taken = on_thread(budget, move || {
        let mut taken = Vec::new();
        assert!(Format::Yaml.parse(&large).is_ok());
        taken.push(reservations_on_this_thread());
        assert!(extract_entry_rows(Format::Yaml, &large).is_ok());
        taken.push(reservations_on_this_thread());
        let key = [PathSeg::Key("key3".into())];
        assert!(resolve_value_span(Format::Yaml, large.as_bytes(), &key).is_some());
        taken.push(reservations_on_this_thread());
        taken
    });
    assert_eq!(taken, [1, 2, 3]);
    assert_eq!(counters(budget), (0, 0, MIB + 1));
}

#[test]
fn the_budget_is_given_back_on_an_error_and_on_a_panic() {
    let budget = budget(2 * MIB);
    let taken = on_thread(budget, move || {
        let large = yaml_of_len(2 * MIB);
        // A syntax error, a refusal by the flow-depth guard, and an extraction
        // that fails.
        let broken = format!("{large}a: [1\n");
        assert!(Format::Yaml.parse(&broken).is_err());
        let deep = format!("{large}a: {}1{}\n", "[".repeat(200), "]".repeat(200));
        assert!(Format::Yaml.parse(&deep).is_err());
        assert!(extract_entry_rows(Format::Yaml, &broken).is_err());
        assert_eq!(counters(budget).0, 0);
        // A panic while a reservation is held.
        let unwound = std::panic::catch_unwind(|| {
            let _held = reserve(large.len());
            panic!("a parser panicked");
        });
        assert!(unwound.is_err());
        assert_eq!(counters(budget).0, 0);
        // The thread no longer counts as holding one, so it takes the next.
        assert!(reserve(large.len()).is_some());
        reservations_on_this_thread()
    });
    assert_eq!(taken, 5);
    assert_eq!(counters(budget), (0, 0, 2 * MIB));
}

#[test]
fn a_nested_reservation_takes_nothing_and_never_waits() {
    let budget = budget(2 * MIB);
    let taken = on_thread(budget, move || {
        let large = yaml_of_len(2 * MIB);
        let outer = reserve(large.len()).unwrap();
        // With the whole budget held by this thread, a nested parse that
        // waited would wait on itself.
        assert!(reserve(large.len()).is_none());
        assert!(Format::Yaml.parse(&large).is_ok());
        drop(outer);
        reservations_on_this_thread()
    });
    assert_eq!(taken, 1);
    assert_eq!(counters(budget), (0, 0, 2 * MIB));
}

#[test]
fn an_input_past_the_budget_takes_all_of_it() {
    let budget = budget(2 * MIB);
    let bytes = on_thread(budget, || reserve(3 * MIB).map(|held| held.bytes));
    assert_eq!(bytes, Some(2 * MIB));
    assert_eq!(counters(budget), (0, 0, 2 * MIB));
}

/// YML-8: a file under the threshold whose aliases expand far past its length
/// costs its expanded tree, so parses of it wait for the budget like those of
/// a large file. The budget charged the length alone, and sixteen files of
/// 500 KB, each with 500 aliases of a string of 500 KB, peaked at 3.1 GB.
#[test]
fn alias_heavy_small_parses_wait_for_their_expanded_size() {
    let budget = budget(2 * MIB);
    let text: &'static str = Box::leak(aliases_of(100_000, 1_000).into_boxed_str());
    let (_, expanded) = crate::yaml_depth::scan_guards(text).expansion_and_size(text);
    let cost = parse_cost(text.len(), expanded);
    assert!(
        text.len() < YAML_BUDGET_THRESHOLD_BYTES && cost > MIB,
        "{cost}"
    );
    let held = with_budget(budget, || reserve(2 * MIB)).unwrap();
    let parses: Vec<_> = (0..2)
        .map(|_| {
            std::thread::spawn(move || {
                with_budget(budget, || {
                    let copies = Format::Yaml
                        .parse(text)
                        .map(|value| value["b"].as_array().map_or(0, Vec::len));
                    (copies, reservations_on_this_thread())
                })
            })
        })
        .collect();
    wait_until("both parses wait", || counters(budget).1 == 2);
    assert!(parses.iter().all(|parse| !parse.is_finished()));
    drop(held);
    wait_until("both parses end", || {
        parses.iter().all(JoinHandle::is_finished)
    });
    for parse in parses {
        let (copies, taken) = parse.join().unwrap();
        assert_eq!(copies, Ok(1_000));
        assert_eq!(taken, 1);
    }
    assert_eq!(counters(budget), (0, 0, 2 * MIB));
}

/// A parse's cost is its length unless the expanded tree is more than
/// [`EXPANSION_BYTES_PER_COST_BYTE`] times that.
#[test]
fn a_parse_costs_its_length_or_its_expansion_s_share() {
    let per = EXPANSION_BYTES_PER_COST_BYTE;
    assert_eq!(parse_cost(10, 0), 10);
    assert_eq!(parse_cost(10, 10 * per), 10);
    assert_eq!(parse_cost(10, 11 * per), 11);
    // A document without aliases is not measured, and costs its length.
    let plain = yaml_of_len(4096);
    let scan = crate::yaml_depth::scan_guards(&plain);
    assert_eq!(scan.expansion_and_size(&plain).1, 0);
    // A few small aliases in an ordinary document (~25 bytes per node, like
    // OpenAPI) stay within the allowance.
    let mut anchored = String::new();
    for i in 0..20 {
        let _ = writeln!(anchored, "entry_{i}: \"ordinary scalar value number {i}\"");
    }
    anchored.push_str("z: &a \"0123456789\"\nrefs: [*a, *a]\n");
    let (_, expanded) = crate::yaml_depth::scan_guards(&anchored).expansion_and_size(&anchored);
    assert!(expanded > 0 && parse_cost(anchored.len(), expanded) == anchored.len());
}

/// A reservation grows by being given back and taken anew, so the thread waits
/// holding nothing; one that holds enough is kept, and a nested one stays
/// empty.
#[test]
fn a_reservation_grows_by_being_taken_anew() {
    let budget = budget(4 * MIB);
    let taken = on_thread(budget, move || {
        let held = reserve(2 * MIB);
        let kept = reserve_at_least(held, MIB).unwrap();
        assert_eq!((kept.bytes, counters(budget).0), (2 * MIB, 2 * MIB));
        let grown = reserve_at_least(Some(kept), 3 * MIB).unwrap();
        assert_eq!((grown.bytes, counters(budget).0), (3 * MIB, 3 * MIB));
        assert!(reserve_at_least(None, 3 * MIB).is_none());
        assert!(reserve_at_least(None, 2 * MIB).is_none());
        drop(grown);
        let from_none = reserve_at_least(None, 2 * MIB).unwrap();
        assert_eq!(from_none.bytes, 2 * MIB);
        drop(from_none);
        reservations_on_this_thread()
    });
    assert_eq!(taken, 3);
    assert_eq!(counters(budget), (0, 0, 3 * MIB));
}
