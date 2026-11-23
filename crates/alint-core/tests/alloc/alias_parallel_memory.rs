//! Peak-heap check for sixteen parallel alias-heavy YAML documents (#582).
//!
//! Each document is ~500 KB (one 500 KB anchored string referenced by 500
//! aliases -> ~250 MB expanded tree). When the YAML parse budget charged only
//! the input length, sixteen parallel parses peaked at ~3.1 GB; retaining each
//! alias-expanded tree's budget lease alongside [`alint_core::SharedValue`]
//! caps concurrent in-flight + live trees to the process budget
//! (`YAML_PARSE_BUDGET_BYTES = 32 MiB`, at most 4 trees at once -> well under
//! 1.6 GiB).

use std::sync::{Arc, Barrier};

use alint_core::Format;

#[global_allocator]
static HEAP: cap::Cap<std::alloc::System> = cap::Cap::new(std::alloc::System, usize::MAX);

const MIB: usize = 1024 * 1024;
/// Upper bound on peak live heap across sixteen parallel 500 KB x 500-alias
/// files (unbounded peaked at ~3.1 GB; bounded to <= 4 concurrent ~250 MB
/// trees stays near ~1.0-1.2 GiB).
const MAX_PARALLEL_HEAP_BYTES: usize = 1800 * MIB;

fn aliases_of(len: usize, copies: usize) -> String {
    let mut text = format!("a: &a \"{}\"\nb:\n", "x".repeat(len));
    for _ in 0..copies {
        text.push_str("  - *a\n");
    }
    text
}

#[test]
fn sixteen_parallel_alias_heavy_files_keep_peak_heap_bounded() {
    let doc = Arc::new(aliases_of(500_000, 500));
    assert!(doc.len() < 1024 * 1024, "input stays below 1 MiB threshold");

    let before = HEAP.allocated();
    let barrier = Arc::new(Barrier::new(16));
    let handles: Vec<_> = (0..16)
        .map(|_| {
            let doc = Arc::clone(&doc);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                let tree = Format::Yaml.parse_shared(&doc).expect("alias doc parses");
                // Simulate a rule inspecting the tree while holding it alive.
                let copies = tree["b"].as_array().map_or(0, Vec::len);
                assert_eq!(copies, 500);
                std::thread::sleep(std::time::Duration::from_millis(5));
                drop(tree);
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }

    let peak_above_baseline = HEAP.max_allocated().saturating_sub(before);
    // In `ALINT_YAML_PARSER=compare` mode (`yaml-compare.sh`), each parse runs
    // ayaml twice, runs libyaml, and formats two full `Debug` strings (~4-5x
    // single-parser heap) while holding the returned value alive. Still run the
    // 16-thread stress check above to catch differential panics, and assert the
    // single-parser peak-heap ceiling outside compare mode.
    let compare_mode = std::env::var("ALINT_YAML_PARSER").as_deref() == Ok("compare");
    if !compare_mode {
        assert!(
            peak_above_baseline <= MAX_PARALLEL_HEAP_BYTES,
            "16 parallel alias-heavy YAML parses peaked at {} MiB (limit {} MiB)",
            peak_above_baseline / MIB,
            MAX_PARALLEL_HEAP_BYTES / MIB
        );
    }
}
