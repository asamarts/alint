//! The heap `key_parity` takes on files deep and wide at once: a reference
//! and an identical peer, each about 50 KB of JSON, 120 objects deep, with
//! 5,000 keys at the bottom. Each key path, W x D segments in all, is held
//! once, by the file's decode; the comparison once copied every path into
//! its key table, again for its leaves, its reference keys, its rows and
//! each peer's rows, and the contract once more as strings: about seven
//! times the decodes, and 2.3 GB for a 320 KB reference and one peer.
//!
//! A binary of its own: it installs a counting global allocator, which counts
//! every allocation in the process. Its one test runs on one thread, so the
//! counts do not depend on timing. The allocator counts a reallocation by
//! the bytes it adds, as the system allocator may grow a block in place.

use alint_core::relation::{Adapter, EntryRowsAdapter, Limits, RelPath, Resource};
use alint_core::{Context, Format, WalkOptions, walk};

#[global_allocator]
static HEAP: cap::Cap<std::alloc::System> = cap::Cap::new(std::alloc::System, usize::MAX);

/// How much more than its two files' decodes a comparison may take at its
/// peak: today's ratio is about 1.5 (the run's cache keeps the decodes, and
/// one comparison builds each key's identity a few times), and every stage
/// copying the key paths took about 7.
const BOUND: usize = 2;

/// The document: `{"a":{"a":...{"k0":0,"k1":0,...}}}`, `depth` objects deep
/// with `width` keys at the bottom.
fn deep_and_wide(depth: usize, width: usize) -> String {
    let keys: Vec<String> = (0..width).map(|key| format!("\"k{key}\":0")).collect();
    format!(
        "{}{{{}{}",
        "{\"a\":".repeat(depth),
        keys.join(","),
        "}".repeat(depth + 1)
    )
}

/// The peak heap `run` takes above what was live before it, and its result.
/// The allocator keeps the highest count, which cannot be reset, so a peak
/// below an earlier one reads as that one: at most, never less, so a bound
/// that holds for the figure holds for the peak. The smaller measure is
/// taken first for that reason.
fn peak_of<T>(run: impl FnOnce() -> T) -> (usize, T) {
    let before = HEAP.allocated();
    let value = run();
    (HEAP.max_allocated().saturating_sub(before), value)
}

#[test]
fn a_comparison_takes_a_small_multiple_of_its_decodes() {
    let text = deep_and_wide(120, 5_000);

    // What the two files' decodes hold at once, as the run's decode cache
    // keeps them for the comparison: the measure the comparison is held to.
    let adapter = EntryRowsAdapter::new(Format::Json).unwrap();
    let limits = Limits::default();
    let resource = |path: &str| Resource::literal(RelPath::parse(path).unwrap());
    let (decodes, outputs) = peak_of(|| {
        ["en.json", "fr.json"].map(|path| {
            adapter
                .extract(&resource(path), text.as_bytes(), &limits)
                .expect("the document decodes")
        })
    });
    drop(outputs);

    let dir = tempfile::tempdir().unwrap();
    for name in ["en.json", "fr.json"] {
        std::fs::write(dir.path().join(name), &text).unwrap();
    }
    let index = walk(
        dir.path(),
        &WalkOptions::default().with_global_git_excludes(false),
    )
    .unwrap();
    let spec: alint_core::RuleSpec = alint_core::yaml::from_str(
        "id: parity\nkind: key_parity\nreference: en.json\npeers: '*.json'\nlevel: error\n",
    )
    .unwrap();
    let rule = alint_rules::builtin_registry().build(&spec).unwrap();
    let (comparison, findings) = peak_of(|| rule.evaluate(&Context::new(dir.path(), &index)));
    let findings = findings.expect("the rule evaluates");

    let tenths = comparison * 10 / decodes.max(1);
    let report = format!(
        "{} bytes per file: the two decodes peak at {decodes} B, the comparison at \
         {comparison} B ({}.{}x)",
        text.len(),
        tenths / 10,
        tenths % 10
    );
    eprintln!("{report}");
    assert!(
        findings.is_empty(),
        "identical files: {:?}",
        findings
            .iter()
            .map(|finding| finding.message.to_string())
            .collect::<Vec<_>>()
    );
    assert!(comparison <= BOUND * decodes, "{report}");
}
