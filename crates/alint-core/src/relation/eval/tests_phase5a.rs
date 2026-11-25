//! Phase 5 Batch 5a evaluation tests (`#593` optional `bound:` and `#598`
//! stub-root memory bounds).

use super::*;
use crate::{WalkOptions, walk};

#[test]
fn optional_bound_skips_resource_or_emits_missing_cell() {
    use crate::relation::plan::BoundMissing;

    let files = tree(&[
        ("root.json", r#"{"deps": {"lodash": "1.0.0"}}"#),
        (
            "pkg-a.json",
            r#"{"name": "pkg-a", "version": "1.0.0", "deps": {"react": "18.0.0"}}"#,
        ),
    ]);

    // `on_missing: skip` skips `root.json` completely (rows AND `resources`),
    // even when a required `bound:` field (`$.version`) is declared BEFORE the
    // optional skip field (`$.name`).
    let skip_rel = named(
        "deps_skip",
        Rows::Part(Part {
            from: from("*.json"),
            reader: Reader::Entries(Entries {
                format: EntriesFormat::Json,
                at: vec!["$.deps".into()],
                leaves: false,
                strict: false,
            }),
            fields: None,
        }),
        &[
            (
                "version",
                FieldSource::Bound {
                    query: "$.version".into(),
                    optional: false,
                    on_missing: BoundMissing::Skip,
                },
            ),
            (
                "pkg",
                FieldSource::Bound {
                    query: "$.name".into(),
                    optional: true,
                    on_missing: BoundMissing::Skip,
                },
            ),
            ("dep", FieldSource::EntryKey),
        ],
    );
    let rows = run(&plan(vec![skip_rel]), 0, &files, Limits::default()).unwrap();
    assert_eq!(rows.rows.len(), 1);
    assert_eq!(rows.resources.len(), 1);
    assert_eq!(rows.resources[0].path().as_str(), "pkg-a.json");
    assert_eq!(
        rows.rows[0].cells[1].value,
        Some(Value::String("pkg-a".into()))
    );
}

#[test]
fn optional_bound_on_missing_missing_keeps_rows_for_entries_and_read() {
    use crate::relation::plan::BoundMissing;

    let files = tree(&[
        ("root.json", r#"{"deps": {"lodash": "1.0.0"}}"#),
        (
            "pkg-a.json",
            r#"{"name": "pkg-a", "version": "1.0.0", "deps": {"react": "18.0.0"}}"#,
        ),
    ]);

    // `on_missing: missing` keeps `root.json`'s row with `None` in the bound cell
    // (both for `entries:` and for `read:`).
    let missing_entries = named(
        "deps_missing",
        Rows::Part(Part {
            from: from("*.json"),
            reader: Reader::Entries(Entries {
                format: EntriesFormat::Json,
                at: vec!["$.deps".into()],
                leaves: false,
                strict: false,
            }),
            fields: None,
        }),
        &[
            (
                "pkg",
                FieldSource::Bound {
                    query: "$.name".into(),
                    optional: true,
                    on_missing: BoundMissing::Missing,
                },
            ),
            ("dep", FieldSource::EntryKey),
        ],
    );
    let rows = run(&plan(vec![missing_entries]), 0, &files, Limits::default()).unwrap();
    assert_eq!(rows.rows.len(), 2);
    assert_eq!(rows.resources.len(), 2);
    assert_eq!(
        rows.rows[0].cells[0].value,
        Some(Value::String("pkg-a".into()))
    );
    assert_eq!(rows.rows[1].cells[0].value, None);
    assert_eq!(
        rows.rows[1].cells[1].value,
        Some(Value::String("lodash".into()))
    );

    let missing_read = named(
        "read_missing",
        Rows::Part(Part {
            from: from("*.json"),
            reader: Reader::Read {
                read: Read::Json {
                    query: "$.deps.*".into(),
                    strict: false,
                },
                optional: false,
            },
            fields: None,
        }),
        &[
            (
                "pkg",
                FieldSource::Bound {
                    query: "$.name".into(),
                    optional: true,
                    on_missing: BoundMissing::Missing,
                },
            ),
            ("ver", FieldSource::Value),
        ],
    );
    let rows = run(&plan(vec![missing_read]), 0, &files, Limits::default()).unwrap();
    assert_eq!(rows.rows.len(), 2);
    assert_eq!(rows.rows[1].cells[0].value, None);
    assert_eq!(
        rows.rows[1].cells[1].value,
        Some(Value::String("1.0.0".into()))
    );
}

#[test]
fn optional_bound_still_fails_on_multi_match_or_non_scalar() {
    use crate::relation::plan::BoundMissing;

    let bad = tree(&[
        ("multi.json", r#"{"names": ["a", "b"], "deps": {"x": "1"}}"#),
        ("obj.json", r#"{"meta": {"a": 1}, "deps": {"x": "1"}}"#),
    ]);
    let multi_rel = named(
        "multi",
        Rows::Part(Part {
            from: from("multi.json"),
            reader: Reader::Entries(Entries {
                format: EntriesFormat::Json,
                at: vec!["$.deps".into()],
                leaves: false,
                strict: false,
            }),
            fields: None,
        }),
        &[(
            "pkg",
            FieldSource::Bound {
                query: "$.names[*]".into(),
                optional: true,
                on_missing: BoundMissing::Skip,
            },
        )],
    );
    let errs = messages(&run(&plan(vec![multi_rel]), 0, &bad, Limits::default()));
    assert!(errs[0].contains("matches more than once"), "{errs:?}");

    let obj_rel = named(
        "obj",
        Rows::Part(Part {
            from: from("obj.json"),
            reader: Reader::Entries(Entries {
                format: EntriesFormat::Json,
                at: vec!["$.deps".into()],
                leaves: false,
                strict: false,
            }),
            fields: None,
        }),
        &[(
            "pkg",
            FieldSource::Bound {
                query: "$.meta".into(),
                optional: true,
                on_missing: BoundMissing::Missing,
            },
        )],
    );
    let errs = messages(&run(&plan(vec![obj_rel]), 0, &bad, Limits::default()));
    assert!(errs[0].contains("a field holds a scalar"), "{errs:?}");
}

#[test]
#[allow(clippy::too_many_lines)]
fn optional_bound_skip_never_swallows_sibling_multi_or_non_scalar_and_runs_before_at_check() {
    use crate::relation::plan::BoundMissing;

    // 1. A multi-match or non-scalar error on one `bound:` field MUST still
    //    fail loud even when a sibling `optional: true, on_missing: skip` field
    //    matches nothing (regardless of declaration order).
    let bad = tree(&[
        ("multi.json", r#"{"tags": ["a", "b"], "deps": {"x": "1"}}"#),
        ("obj.json", r#"{"meta": {"k": 1}, "deps": {"x": "1"}}"#),
    ]);
    let multi_rel = named(
        "multi_with_skip",
        Rows::Part(Part {
            from: from("multi.json"),
            reader: Reader::Entries(Entries {
                format: EntriesFormat::Json,
                at: vec!["$.deps".into()],
                leaves: false,
                strict: false,
            }),
            fields: None,
        }),
        &[
            (
                "pkg",
                FieldSource::Bound {
                    query: "$.name".into(),
                    optional: true,
                    on_missing: BoundMissing::Skip,
                },
            ),
            (
                "tags",
                FieldSource::Bound {
                    query: "$.tags[*]".into(),
                    optional: true,
                    on_missing: BoundMissing::Skip,
                },
            ),
        ],
    );
    let errs = messages(&run(&plan(vec![multi_rel]), 0, &bad, Limits::default()));
    assert!(errs[0].contains("matches more than once"), "{errs:?}");

    let obj_rel = named(
        "obj_with_skip",
        Rows::Part(Part {
            from: from("obj.json"),
            reader: Reader::Entries(Entries {
                format: EntriesFormat::Json,
                at: vec!["$.deps".into()],
                leaves: false,
                strict: false,
            }),
            fields: None,
        }),
        &[
            (
                "pkg",
                FieldSource::Bound {
                    query: "$.name".into(),
                    optional: true,
                    on_missing: BoundMissing::Skip,
                },
            ),
            (
                "meta",
                FieldSource::Bound {
                    query: "$.meta".into(),
                    optional: true,
                    on_missing: BoundMissing::Skip,
                },
            ),
        ],
    );
    let errs = messages(&run(&plan(vec![obj_rel]), 0, &bad, Limits::default()));
    assert!(errs[0].contains("a field holds a scalar"), "{errs:?}");

    // 2. When a resource lacks an `optional: true, on_missing: skip` bound
    //    field (and has no multi/non-scalar bound errors), `entries:` skips it
    //    cleanly BEFORE checking `at:` selects objects (matching `read:`).
    let mixed = tree(&[
        ("nameless.json", r#"{"exports": "./index.js"}"#),
        (
            "named.json",
            r#"{"name": "pkg", "exports": {".": "./index.js"}}"#,
        ),
    ]);
    let rel = named(
        "exports",
        Rows::Part(Part {
            from: from("*.json"),
            reader: Reader::Entries(Entries {
                format: EntriesFormat::Json,
                at: vec!["$.exports".into()],
                leaves: false,
                strict: false,
            }),
            fields: None,
        }),
        &[
            (
                "pkg",
                FieldSource::Bound {
                    query: "$.name".into(),
                    optional: true,
                    on_missing: BoundMissing::Skip,
                },
            ),
            ("sub", FieldSource::EntryKey),
        ],
    );
    let rows = run(&plan(vec![rel]), 0, &mixed, Limits::default()).unwrap();
    assert_eq!(rows.rows.len(), 1);
    assert_eq!(rows.resources.len(), 1);
    assert_eq!(rows.resources[0].path().as_str(), "named.json");
}

#[test]
fn stub_rows_shrink_to_fit_drop_without_entry_root_and_stay_within_memory_bound() {
    let mut files = vec![("l000.yml".to_string(), "l000:\n  k: v\n".to_string())];
    for i in 1..40 {
        files.push((format!("l{i:03}.yml"), format!("l{i:03}: {{}}\n")));
    }
    let refs: Vec<(&str, &str)> = files
        .iter()
        .map(|(p, c)| (p.as_str(), c.as_str()))
        .collect();
    let files_tree = tree(&refs);

    // With `entry.root` + `where:` keeping only 1 stub: `stub_rows` shrinks to fit
    // and stays within the run memory bound.
    let mut with_root = named(
        "with_root",
        Rows::Part(Part {
            from: from("*.yml"),
            reader: Reader::Entries(Entries {
                format: EntriesFormat::Yaml,
                at: vec!["$".into()],
                leaves: true,
                strict: false,
            }),
            fields: None,
        }),
        &[
            ("locale", FieldSource::EntryRoot),
            ("key", FieldSource::EntryPath),
        ],
    );
    with_root.relation.transforms = vec![("key".into(), vec![Transform::StripRoot])];
    with_root.relation.filters = vec![(
        "locale".into(),
        vec![Predicate::In(vec![Member::Literal(Value::String(
            "l001".into(),
        ))])],
    )];

    // Without `entry.root`: `stub_rows` is cleared completely (0 retained).
    let mut without_root = named(
        "without_root",
        Rows::Part(Part {
            from: from("*.yml"),
            reader: Reader::Entries(Entries {
                format: EntriesFormat::Yaml,
                at: vec!["$".into()],
                leaves: true,
                strict: false,
            }),
            fields: None,
        }),
        &[
            ("file", FieldSource::Path(PathToken::Stem)),
            ("key", FieldSource::EntryPath),
        ],
    );
    without_root.relation.transforms = vec![("key".into(), vec![Transform::StripRoot])];

    let plan = plan(vec![with_root, without_root]);
    let index = walk(
        files_tree.dir.path(),
        &WalkOptions::default().with_global_git_excludes(false),
    )
    .unwrap();
    let ctx = Context::new(files_tree.dir.path(), &index);
    let r0 = super::super::evaluate(&plan, RelationId(0), &ctx).unwrap();
    let r1 = super::super::evaluate(&plan, RelationId(1), &ctx).unwrap();
    assert_eq!(r0.stub_rows.len(), 1);
    assert_eq!(r0.stub_rows.capacity(), 1);
    assert_eq!(r1.stub_rows.len(), 0);
    assert_eq!(r1.stub_rows.capacity(), 0);
    let memo = index.relation_memo(&plan).unwrap();
    assert_held_within_the_bound(&memo);
}
