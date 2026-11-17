//! The heap the relation layer's entry extraction takes on a document deep
//! and wide at once: about 1 MB of JSON, 120 objects deep, with 100,000 keys
//! at the bottom. Every row once copied its whole key path, W x D segments
//! in all: a relation whose `at:` path selected nothing peaked at 40 times a
//! parse of the document to a tree (about 400 MiB here), and `key_parity`'s
//! entry rows at about 700 MiB, outside every relation limit.
//!
//! A binary of its own: it installs a counting global allocator, which counts
//! every allocation in the process. Its one test runs on one thread, so the
//! counts do not depend on timing. The allocator counts a reallocation by
//! the bytes it adds, as the system allocator may grow a block in place.

use std::sync::Arc;

use alint_core::relation::eval::evaluate;
use alint_core::relation::plan::{
    Entries, EntriesFormat, FieldDef, FieldSource, NamedRelation, Part, Plan, PlanSource, Reader,
    RelationDef, RelationId, Rows, SelectorDef, SelectorRef, SourceIndex,
};
use alint_core::relation::{Adapter, AdapterError, EntryRowsAdapter, LimitKind, Limits, RelPath};
use alint_core::{ConfigSourceClass, Context, Format, PathsSpec, WalkOptions, walk};

#[global_allocator]
static HEAP: cap::Cap<std::alloc::System> = cap::Cap::new(std::alloc::System, usize::MAX);

/// How much more than a parse of the document to a tree an extraction may
/// take at its peak: today's ratio is about 4 (the relation parses the
/// document twice, to its entries and to a tree for its `JSONPath`s), and
/// every row copying its key path took 40 for the relation and 68 for the
/// key rows.
const BOUND: usize = 8;

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
/// that holds for the figure holds for the peak.
fn peak_of<T>(run: impl FnOnce() -> T) -> (usize, T) {
    let before = HEAP.allocated();
    let value = run();
    (HEAP.max_allocated().saturating_sub(before), value)
}

fn relation(at: &str) -> Arc<Plan> {
    Arc::new(Plan {
        sources: vec![PlanSource {
            display: ".alint.yml".into(),
            path: ".alint.yml".into(),
            class: ConfigSourceClass::Root,
            aliases: Vec::new(),
        }],
        selectors: Vec::new(),
        relations: vec![NamedRelation {
            name: "deep".into(),
            source: SourceIndex(0),
            relation: RelationDef {
                rows: Rows::Part(Part {
                    from: SelectorRef::Inline(Box::new(SelectorDef {
                        paths: PathsSpec::Single("deep.json".into()),
                        scope_filter: None,
                    })),
                    reader: Reader::Entries(Entries {
                        format: EntriesFormat::Json,
                        at: vec![at.into()],
                        leaves: false,
                    }),
                }),
                fields: vec![(
                    "key".into(),
                    FieldDef {
                        source: FieldSource::EntryKey,
                        sensitive: false,
                    },
                )],
                transforms: Vec::new(),
                filters: Vec::new(),
            },
            capabilities: alint_core::relation::plan::Capabilities::default(),
            reads_environment: false,
        }],
        rules: Vec::new(),
    })
}

#[test]
fn entry_rows_take_a_small_multiple_of_a_parse_or_fail_at_a_limit() {
    let text = deep_and_wide(120, 100_000);
    // What a parse to a tree takes, which every structured read pays: the
    // measure the extractions are held to.
    let (tree, parsed) = peak_of(|| Format::Json.parse(&text));
    drop(parsed.expect("the document parses"));

    // A relation whose `at:` path selects nothing holds no key path.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("deep.json"), &text).unwrap();
    let index = walk(dir.path(), &WalkOptions::default()).unwrap();
    let plan = relation("$.version");
    let (relation, outcome) =
        peak_of(|| evaluate(&plan, RelationId(0), &Context::new(dir.path(), &index)));
    let rows = outcome.expect("the relation evaluates");
    assert_eq!(rows.rows.len(), 0);
    drop(rows);

    // `key_parity`'s entry rows build every key path: past the run's bytes,
    // the decode fails as it reaches them.
    let resource = alint_core::relation::Resource::literal(RelPath::parse("deep.json").unwrap());
    let limits = Limits::default()
        .with(LimitKind::BytesPerRun, 8 << 20)
        .unwrap();
    let (key_rows, decoded) = peak_of(|| {
        EntryRowsAdapter::new(Format::Json)
            .unwrap()
            .extract(&resource, text.as_bytes(), &limits)
    });
    let failed = match &decoded {
        Err(AdapterError::Limit(limit)) => limit.kind == LimitKind::BytesPerRun,
        _ => false,
    };
    let rows = decoded.map_or(0, |output| output.rows.len());

    let report = format!(
        "{} bytes: a parse to a tree peaks at {tree} B; a relation selecting nothing at \
         {relation} B; key rows {} at {key_rows} B",
        text.len(),
        if failed {
            "failing at 8 MiB of key paths".to_string()
        } else {
            format!("not failing at 8 MiB of key paths ({rows} rows)")
        }
    );
    eprintln!("{report}");
    assert!(relation <= BOUND * tree, "{report}");
    assert!(failed, "{report}");
    assert!(key_rows <= BOUND * tree + (8 << 20), "{report}");
}
