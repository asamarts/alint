//! The heap placing a finding takes: finding where a `read:` value or an
//! entry's key is written in its file, read again, takes the document's
//! parse and its entries, and the figure placing reports for it
//! ([`find_spans`]) counts them, so the run's byte budget bounds them.
//! Its figure once missed the parse its entries are read from (a JSON
//! syntax tree, YAML's events), most of what placing held: a 13.6 MB JSON
//! file held about 100 MB more to place one value.
//!
//! A binary of its own: it installs a counting global allocator, which counts
//! every allocation in the process. Its one test runs on one thread, so the
//! counts do not depend on timing.

use std::sync::Arc;

use alint_core::relation::eval::{Span, evaluate, find_spans};
use alint_core::relation::plan::{
    Entries, EntriesFormat, FieldDef, FieldSource, NamedRelation, Part, Plan, PlanSource, Read,
    Reader, RelationDef, RelationId, Rows, SelectorDef, SelectorRef, SourceIndex,
};
use alint_core::{ConfigSourceClass, Context, PathsSpec, WalkOptions, walk};

#[global_allocator]
static HEAP: cap::Cap<std::alloc::System> = cap::Cap::new(std::alloc::System, usize::MAX);

/// How far the figure may fall below what placing took at its peak.
const FACTOR: usize = 2;

/// A plan of one relation, reading `doc` with `reader` into one field read
/// from `source`.
fn relation(doc: &str, reader: Reader, source: FieldSource) -> Arc<Plan> {
    Arc::new(Plan {
        sources: vec![PlanSource {
            display: ".alint.yml".into(),
            path: ".alint.yml".into(),
            class: ConfigSourceClass::Root,
            aliases: Vec::new(),
        }],
        selectors: Vec::new(),
        relations: vec![NamedRelation {
            name: "r".into(),
            source: SourceIndex(0),
            relation: RelationDef {
                rows: Rows::Part(Part {
                    from: SelectorRef::Inline(Box::new(SelectorDef {
                        paths: PathsSpec::Single(doc.into()),
                        scope_filter: None,
                    })),
                    reader,
                }),
                fields: vec![(
                    "f".into(),
                    FieldDef {
                        source,
                        sensitive: false,
                    },
                )],
                transforms: Vec::new(),
                filters: Vec::new(),
            },
            capabilities: alint_core::relation::plan::Capabilities::default(),
            reads_environment: false,
            definition_reads_environment: false,
        }],
        rules: Vec::new(),
    })
}

/// The span of the first row's cell `reader` reads from `small`, a document
/// that holds what `large` holds first: placing it in `large` finds it at
/// the same row. Read from the small one, so the read's own peak is low.
fn span(name: &str, small: &str, reader: Reader, source: FieldSource) -> Span {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(name), small).unwrap();
    let index = walk(dir.path(), &WalkOptions::default()).unwrap();
    let plan = relation(name, reader, source);
    let rows = evaluate(&plan, RelationId(0), &Context::new(dir.path(), &index))
        .expect("the relation evaluates");
    rows.rows[0].cells[0].span.clone().expect("a span")
}

/// A document of `format`: `{"target": "t", "k0": "v0", ...}`, `width`
/// members.
fn wide(format: &str, width: usize) -> String {
    let member = |key: &str, value: &str| match format {
        "json" => format!("\"{key}\": \"{value}\""),
        "yaml" => format!("{key}: \"{value}\""),
        _ => format!("{key} = \"{value}\""),
    };
    let mut members = vec![member("target", "t")];
    members.extend((0..width).map(|index| member(&format!("k{index}"), &format!("v{index}"))));
    match format {
        "json" => format!("{{{}}}\n", members.join(",\n")),
        _ => format!("{}\n", members.join("\n")),
    }
}

/// The span of the first row a `read:` (`entry` false) or an `entries:`
/// part reads from `small`, a document of `format`.
fn first_span(format: &str, small: &str, entry: bool) -> Span {
    let name = format!("a.{format}");
    if entry {
        let format = match format {
            "json" => EntriesFormat::Json,
            "yaml" => EntriesFormat::Yaml,
            _ => EntriesFormat::Toml,
        };
        let reader = Reader::Entries(Entries {
            format,
            at: vec!["$".into()],
            leaves: false,
        });
        span(&name, small, reader, FieldSource::EntryKey)
    } else {
        let read = match format {
            "json" => Read::Json("$.target".into()),
            "yaml" => Read::Yaml("$.target".into()),
            _ => Read::Toml("$.target".into()),
        };
        let reader = Reader::Read {
            read,
            optional: false,
        };
        span(&name, small, reader, FieldSource::Value)
    }
}

#[test]
fn placing_counts_the_parse_it_reads_entries_from() {
    for (format, entry, width) in [
        ("json", false, 500_000),
        ("json", true, 500_000),
        ("toml", false, 60_000),
        ("toml", true, 60_000),
        ("yaml", false, 100_000),
        ("yaml", true, 100_000),
    ] {
        let span = first_span(format, &wide(format, 1), entry);
        let large = wide(format, width);
        // The allocator keeps the highest count, which cannot be reset: an
        // allocation never written to (so never paged in) lifts the count
        // to it, so placing's own peak passes it and is measured.
        let high = HEAP.max_allocated();
        let ballast: Vec<u8> = Vec::with_capacity(high - HEAP.allocated());
        let before = HEAP.allocated();
        let (found, held) = find_spans(large.as_bytes(), [&span]);
        let peak = HEAP.max_allocated() - before;
        drop(ballast);
        let line = format!(
            "{format} {}, {} bytes: placing peaks at {peak} B, counts {held} B",
            if entry { "entry" } else { "read" },
            large.len()
        );
        eprintln!("{line}");
        assert!(found[0].is_some(), "{line}");
        assert!(HEAP.max_allocated() > high, "not measured: {line}");
        assert!(held * FACTOR >= peak, "{line}");
    }
}
