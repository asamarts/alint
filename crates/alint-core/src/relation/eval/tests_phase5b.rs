//! Phase 5 Batch 5b evaluation tests (ADR-0050 D8 / #592):
//! - `records:` array-of-objects extraction across JSON, YAML, and TOML with
//!   exact source spans and preserved numeric lexemes;
//! - parameterized string transforms (`split`, `strip_prefix`, `strip_suffix`,
//!   `prefix`, `suffix`) including negative indices, optional vs required
//!   out-of-range splits, and `BytesPerRun` charging;
//! - mixed-reader `union:` with per-part `fields:` and `keys_sensitive`.

#![allow(clippy::too_many_lines)]

use super::*;
use crate::relation::plan::{Capabilities, Records};

fn records_part(paths: &str, format: EntriesFormat, at: &[&str]) -> Part {
    Part {
        from: from(paths),
        reader: Reader::Records(Records {
            format,
            at: at.iter().map(|s| (*s).to_string()).collect(),
            strict: false,
        }),
        fields: None,
    }
}

#[test]
fn records_extracts_json_yaml_and_toml_with_spans_and_number_lexemes() {
    let files = tree(&[
        (
            "a.json",
            "{\n  \"items\": [\n    {\"name\": \"api\", \"port\": 8080, \"ratio\": 1.00},\n    \
             {\"name\": \"web\", \"port\": 3000}\n  ]\n}\n",
        ),
        (
            "b.yaml",
            "spec:\n  containers:\n    - name: worker\n      image: ghcr.io/org/app:1.2.3\n      \
             replicas: 2.0\n    - name: sidecar\n      image: nginx\n",
        ),
        (
            "c.toml",
            "[[services]]\nname = \"db\"\nport = 5432\n\n[[services]]\nname = \"cache\"\n",
        ),
    ]);

    let json_rel = named(
        "json_rec",
        Rows::Part(records_part("a.json", EntriesFormat::Json, &["$.items[*]"])),
        &[
            (
                "name",
                FieldSource::Record {
                    query: "$.name".into(),
                    optional: false,
                },
            ),
            (
                "ratio",
                FieldSource::Record {
                    query: "$.ratio".into(),
                    optional: true,
                },
            ),
        ],
    );
    let yaml_rel = named(
        "yaml_rec",
        Rows::Part(records_part(
            "b.yaml",
            EntriesFormat::Yaml,
            &["$.spec.containers[*]"],
        )),
        &[
            (
                "name",
                FieldSource::Record {
                    query: "$.name".into(),
                    optional: false,
                },
            ),
            (
                "replicas",
                FieldSource::Record {
                    query: "$.replicas".into(),
                    optional: true,
                },
            ),
        ],
    );
    let toml_rel = named(
        "toml_rec",
        Rows::Part(records_part(
            "c.toml",
            EntriesFormat::Toml,
            &["$.services[*]"],
        )),
        &[
            (
                "name",
                FieldSource::Record {
                    query: "$.name".into(),
                    optional: false,
                },
            ),
            (
                "port",
                FieldSource::Record {
                    query: "$.port".into(),
                    optional: true,
                },
            ),
        ],
    );
    let plan = plan(vec![json_rel, yaml_rel, toml_rel]);

    // JSON: preserved lexeme `1.00` (not collapsed to `1`) + missing optional cell + spans.
    let jr = run(&plan, 0, &files, Limits::default()).unwrap();
    assert_eq!(jr.rows.len(), 2);
    assert_eq!(jr.rows[0].cells[0].value, Some(Value::String("api".into())));
    assert!(jr.rows[0].cells[0].span.is_some());
    assert_eq!(
        jr.rows[0].cells[1].value,
        Some(Value::Number(
            crate::relation::Number::from_lexeme("1.00").unwrap()
        ))
    );
    assert!(jr.rows[0].cells[1].span.is_some());
    assert_eq!(jr.rows[1].cells[0].value, Some(Value::String("web".into())));
    assert_eq!(jr.rows[1].cells[1].value, None);

    // YAML: preserved lexeme `2.0` + exact spans + missing optional cell.
    let yr = run(&plan, 1, &files, Limits::default()).unwrap();
    assert_eq!(yr.rows.len(), 2);
    assert_eq!(
        yr.rows[0].cells[0].value,
        Some(Value::String("worker".into()))
    );
    assert!(yr.rows[0].cells[0].span.is_some());
    assert_eq!(
        yr.rows[0].cells[1].value,
        Some(Value::Number(
            crate::relation::Number::from_lexeme("2.0").unwrap()
        ))
    );
    assert!(yr.rows[0].cells[1].span.is_some());
    assert_eq!(yr.rows[1].cells[1].value, None);

    // TOML: array-of-tables extraction + exact spans + missing optional cell.
    let tr = run(&plan, 2, &files, Limits::default()).unwrap();
    assert_eq!(tr.rows.len(), 2);
    assert_eq!(tr.rows[0].cells[0].value, Some(Value::String("db".into())));
    assert!(tr.rows[0].cells[0].span.is_some());
    assert_eq!(
        tr.rows[0].cells[1].value,
        Some(Value::Number(
            crate::relation::Number::from_lexeme("5432").unwrap()
        ))
    );
    assert!(tr.rows[0].cells[1].span.is_some());
    assert_eq!(tr.rows[1].cells[1].value, None);
}

#[test]
fn records_fails_loud_on_non_object_missing_required_multi_match_and_non_scalar() {
    let files = tree(&[
        ("non_obj.json", r#"{"items": ["not-an-object"]}"#),
        ("missing_field.json", r#"{"items": [{"name": "a"}]}"#),
        ("multi.json", r#"{"items": [{"tags": ["x", "y"]}]}"#),
        ("non_scalar.json", r#"{"items": [{"meta": {"a": 1}}]}"#),
        ("absent_at.json", r#"{"other": []}"#),
    ]);

    let non_obj = named(
        "non_obj",
        Rows::Part(records_part(
            "non_obj.json",
            EntriesFormat::Json,
            &["$.items[*]"],
        )),
        &[(
            "name",
            FieldSource::Record {
                query: "$.name".into(),
                optional: false,
            },
        )],
    );
    let errs = messages(&run(&plan(vec![non_obj]), 0, &files, Limits::default()));
    assert!(
        errs[0].contains("selects a string at `items[0]`"),
        "expected non-object failure: {errs:?}"
    );

    let missing_req = named(
        "missing_req",
        Rows::Part(records_part(
            "missing_field.json",
            EntriesFormat::Json,
            &["$.items[*]"],
        )),
        &[(
            "image",
            FieldSource::Record {
                query: "$.image".into(),
                optional: false,
            },
        )],
    );
    let errs = messages(&run(&plan(vec![missing_req]), 0, &files, Limits::default()));
    assert!(
        errs[0].contains("matches nothing"),
        "expected missing required record field failure: {errs:?}"
    );

    let multi = named(
        "multi",
        Rows::Part(records_part(
            "multi.json",
            EntriesFormat::Json,
            &["$.items[*]"],
        )),
        &[(
            "tag",
            FieldSource::Record {
                query: "$.tags[*]".into(),
                optional: true,
            },
        )],
    );
    let errs = messages(&run(&plan(vec![multi]), 0, &files, Limits::default()));
    assert!(
        errs[0].contains("matches more than once"),
        "expected multi-match failure: {errs:?}"
    );

    let non_scalar = named(
        "non_scalar",
        Rows::Part(records_part(
            "non_scalar.json",
            EntriesFormat::Json,
            &["$.items[*]"],
        )),
        &[(
            "meta",
            FieldSource::Record {
                query: "$.meta".into(),
                optional: true,
            },
        )],
    );
    let errs = messages(&run(&plan(vec![non_scalar]), 0, &files, Limits::default()));
    assert!(
        errs[0].contains("a field holds a scalar"),
        "expected non-scalar failure: {errs:?}"
    );

    // Absent `at:` yields 0 rows cleanly.
    let absent = named(
        "absent",
        Rows::Part(records_part(
            "absent_at.json",
            EntriesFormat::Json,
            &["$.items[*]"],
        )),
        &[(
            "name",
            FieldSource::Record {
                query: "$.name".into(),
                optional: false,
            },
        )],
    );
    let rows = run(&plan(vec![absent]), 0, &files, Limits::default()).unwrap();
    assert_eq!(rows.rows.len(), 0);
}

#[test]
fn parameterized_transforms_handle_split_affixes_missing_required_and_bytes_budget() {
    let files = tree(&[(
        "imgs.txt",
        "ghcr.io/org/app:1.2.3\nnginx\n$FOO.plural\n$BAR.singular\n",
    )]);

    // `split` with negative index `-1`, `part: 1` (optional -> Missing on "nginx"),
    // `strip_prefix`, `strip_suffix` (list, first matching once), `prefix`, `suffix`.
    let mut rel = named(
        "imgs",
        Rows::Part(lines("imgs.txt")),
        &[
            ("last", FieldSource::Value),
            ("tag", FieldSource::Value),
            ("clean", FieldSource::Value),
        ],
    );
    rel.relation.transforms = vec![
        (
            "last".into(),
            vec![Transform::Split {
                on: ":".into(),
                part: -1,
                required: false,
            }],
        ),
        (
            "tag".into(),
            vec![Transform::Split {
                on: ":".into(),
                part: 1,
                required: false,
            }],
        ),
        (
            "clean".into(),
            vec![
                Transform::StripPrefix("$".into()),
                Transform::StripSuffix(vec![".singular".into(), ".plural".into()]),
                Transform::Prefix("p-".into()),
                Transform::Suffix("-s".into()),
            ],
        ),
    ];
    let rows = run(&plan(vec![rel]), 0, &files, Limits::default()).unwrap();
    let vals = |col: usize| -> Vec<Option<String>> {
        rows.rows
            .iter()
            .map(|r| match &r.cells[col].value {
                Some(Value::String(s)) => Some(s.to_string()),
                None => None,
                other => panic!("unexpected {other:?}"),
            })
            .collect()
    };
    assert_eq!(
        vals(0),
        vec![
            Some("1.2.3".into()),
            Some("nginx".into()),
            Some("$FOO.plural".into()),
            Some("$BAR.singular".into()),
        ]
    );
    assert_eq!(vals(1), vec![Some("1.2.3".into()), None, None, None]);
    assert_eq!(
        vals(2),
        vec![
            Some("p-ghcr.io/org/app:1.2.3-s".into()),
            Some("p-nginx-s".into()),
            Some("p-FOO-s".into()),
            Some("p-BAR-s".into()),
        ]
    );

    // `split` with `required: true` fails loud when out of range.
    let mut req = named(
        "req",
        Rows::Part(lines("imgs.txt")),
        &[("tag", FieldSource::Value)],
    );
    req.relation.transforms = vec![(
        "tag".into(),
        vec![Transform::Split {
            on: ":".into(),
            part: 1,
            required: true,
        }],
    )];
    let errs = messages(&run(&plan(vec![req]), 0, &files, Limits::default()));
    assert!(
        errs[0].contains("part 1 is out of range"),
        "expected required split failure: {errs:?}"
    );

    // `prefix` expanding strings charges `BytesPerRun`.
    let mut big = named(
        "big",
        Rows::Part(lines("imgs.txt")),
        &[("tag", FieldSource::Value)],
    );
    big.relation.transforms = vec![("tag".into(), vec![Transform::Prefix("x".repeat(4096))])];
    let errs = messages(&run(
        &plan(vec![big]),
        0,
        &files,
        limit(LimitKind::BytesPerRun, 2048),
    ));
    assert!(
        errs[0].contains("`bytes_per_run`"),
        "expected bytes budget failure: {errs:?}"
    );
}

#[test]
fn mixed_reader_union_per_part_fields_align_columns_and_track_keys_sensitive() {
    let files = tree(&[
        ("a.json", r#"{"commands": {"help": "Show help"}}"#),
        ("b.json", r#"{"aliases": ["h", "?"]}"#),
        ("c.yaml", "items:\n  - name: status\n"),
    ]);
    let field_def = |source: FieldSource, sensitive: bool| FieldDef { source, sensitive };
    let union_rel = NamedRelation {
        name: "names".into(),
        source: SourceIndex(0),
        relation: RelationDef {
            rows: Rows::Union(vec![
                Part {
                    from: from("a.json"),
                    reader: Reader::Entries(Entries {
                        format: EntriesFormat::Json,
                        at: vec!["$.commands".into()],
                        leaves: false,
                        strict: false,
                    }),
                    fields: Some(vec![
                        ("name".into(), field_def(FieldSource::EntryKey, false)),
                        (
                            "file".into(),
                            field_def(FieldSource::Path(PathToken::Path), false),
                        ),
                    ]),
                },
                Part {
                    from: from("b.json"),
                    reader: Reader::Read {
                        read: Read::Json {
                            query: "$.aliases[*]".into(),
                            strict: false,
                        },
                        optional: false,
                    },
                    fields: Some(vec![
                        ("name".into(), field_def(FieldSource::Value, false)),
                        (
                            "file".into(),
                            field_def(FieldSource::Path(PathToken::Path), false),
                        ),
                    ]),
                },
                Part {
                    from: from("c.yaml"),
                    reader: Reader::Records(Records {
                        format: EntriesFormat::Yaml,
                        at: vec!["$.items[*]".into()],
                        strict: false,
                    }),
                    fields: Some(vec![
                        (
                            "name".into(),
                            field_def(
                                FieldSource::Record {
                                    query: "$.name".into(),
                                    optional: false,
                                },
                                false,
                            ),
                        ),
                        (
                            "file".into(),
                            field_def(FieldSource::Path(PathToken::Path), false),
                        ),
                    ]),
                },
            ]),
            fields: vec![
                ("name".into(), field_def(FieldSource::EntryKey, false)),
                (
                    "file".into(),
                    field_def(FieldSource::Path(PathToken::Path), false),
                ),
            ],
            transforms: Vec::new(),
            filters: Vec::new(),
        },
        capabilities: Capabilities::default(),
        reads_environment: false,
        definition_reads_environment: false,
    };

    let plan_val = plan(vec![union_rel.clone()]);
    let rows = run(&plan_val, 0, &files, Limits::default()).unwrap();
    let pairs: Vec<(String, String)> = rows
        .rows
        .iter()
        .map(|r| (text(&r.cells[0]).to_string(), text(&r.cells[1]).to_string()))
        .collect();
    assert_eq!(
        pairs,
        vec![
            ("help".into(), "a.json".into()),
            ("h".into(), "b.json".into()),
            ("?".into(), "b.json".into()),
            ("status".into(), "c.yaml".into()),
        ]
    );
    assert!(!super::super::keys_sensitive(&plan_val, RelationId(0)));

    // Marking `name` sensitive propagates `keys_sensitive` when part 0 binds `entry.key`.
    let mut sens_rel = union_rel;
    sens_rel.relation.fields[0].1.sensitive = true;
    if let Rows::Union(parts) = &mut sens_rel.relation.rows {
        for part in parts {
            if let Some(fields) = &mut part.fields {
                fields[0].1.sensitive = true;
            }
        }
    }
    let sens_plan = plan(vec![sens_rel]);
    assert!(super::super::keys_sensitive(&sens_plan, RelationId(0)));
}

#[test]
fn records_empty_objects_stay_linear_empty_root_zero_rows_and_non_object_stops_early() {
    // 2,000 empty `{}` objects stay O(N) via single-pass `locate_cached`.
    let many_empty = format!(
        r#"{{"items":[{}]}}"#,
        std::iter::repeat_n("{}", 2_000)
            .collect::<Vec<_>>()
            .join(",")
    );
    let files_many = tree(&[("many.json", many_empty.as_str())]);
    let plan_empty_elems = plan(vec![named(
        "many",
        Rows::Part(records_part(
            "many.json",
            EntriesFormat::Json,
            &["$.items[*]"],
        )),
        &[(
            "name",
            FieldSource::Record {
                query: "$.name".into(),
                optional: true,
            },
        )],
    )]);
    let rows = run(&plan_empty_elems, 0, &files_many, Limits::default()).unwrap();
    assert_eq!(rows.rows.len(), 2_000);
    assert!(rows.rows.iter().all(|r| r.cells[0].value.is_none()));

    // Empty / whitespace / BOM-only files on `at: "$"` emit 0 rows (matching
    // comment-only YAML) instead of failing or emitting a phantom row.
    for (name, fmt, body) in [
        ("empty.json", EntriesFormat::Json, "   \n"),
        ("bom.json", EntriesFormat::Json, "\u{feff}  "),
        ("empty.yaml", EntriesFormat::Yaml, "  \n"),
        ("comment.yaml", EntriesFormat::Yaml, "# comment only\n"),
        ("empty.toml", EntriesFormat::Toml, "  \n"),
    ] {
        let files = tree(&[(name, body)]);
        let p = plan(vec![named(
            "root",
            Rows::Part(records_part(name, fmt, &["$"])),
            &[(
                "name",
                FieldSource::Record {
                    query: "$.name".into(),
                    optional: false,
                },
            )],
        )]);
        let rows = run(&p, 0, &files, Limits::default()).unwrap();
        assert!(rows.rows.is_empty(), "{name} should emit 0 rows");
    }

    // Non-object first element stops `extract_at` immediately: 3 trailing
    // objects fit under `RowsPerExtractor = 5` during `entry_table_with_arrays`
    // (4 JSON keys total), but would exceed 5 adapter rows (1 error + 3*2 = 7)
    // if `extract_at` kept extracting after `"bad"`.
    let trailing_objs = format!(
        r#"{{"items":["bad",{}]}}"#,
        std::iter::repeat_n(r#"{"name":"x"}"#, 3)
            .collect::<Vec<_>>()
            .join(",")
    );
    let files_bad = tree(&[("bad.json", trailing_objs.as_str())]);
    let p_non_obj = plan(vec![named(
        "bad",
        Rows::Part(records_part(
            "bad.json",
            EntriesFormat::Json,
            &["$.items[*]"],
        )),
        &[(
            "name",
            FieldSource::Record {
                query: "$.name".into(),
                optional: false,
            },
        )],
    )]);
    let errs = messages(&run(
        &p_non_obj,
        0,
        &files_bad,
        limit(LimitKind::RowsPerExtractor, 5),
    ));
    assert!(
        errs.iter().any(|m| m.contains("selects a string")),
        "must report non-object error rather than RowsPerExtractor: {errs:?}"
    );
    assert!(
        errs.iter()
            .any(|m| m.contains("each of which becomes a row")),
        "records: advice must describe record objects: {errs:?}"
    );
}

#[test]
fn records_exact_spans_verify_through_find_spans_and_toml_entries_stay_isolated() {
    use std::fmt::Write as _;

    use super::super::spans::find_spans;

    let json_text = r#"{"items":[{"name":"api","port":8080}]}"#;
    let yaml_text = "items:\n  - name: api\n    port: 8080\n";
    let toml_text = "[[items]]\nname = \"api\"\nport = 8080\n";
    for (file, fmt, body) in [
        ("a.json", EntriesFormat::Json, json_text),
        ("a.yaml", EntriesFormat::Yaml, yaml_text),
        ("a.toml", EntriesFormat::Toml, toml_text),
    ] {
        let files = tree(&[(file, body)]);
        let p = plan(vec![named(
            "r",
            Rows::Part(records_part(file, fmt, &["$.items[*]"])),
            &[
                (
                    "name",
                    FieldSource::Record {
                        query: "$.name".into(),
                        optional: false,
                    },
                ),
                (
                    "port",
                    FieldSource::Record {
                        query: "$.port".into(),
                        optional: false,
                    },
                ),
            ],
        )]);
        let rows = run(&p, 0, &files, Limits::default()).unwrap();
        let name_span = rows.rows[0].cells[0].span.as_ref().expect("name span");
        let port_span = rows.rows[0].cells[1].span.as_ref().expect("port span");
        let (found, _) = find_spans(body.as_bytes(), [name_span, port_span]);
        let name_bs = found[0].expect("verified name span");
        let port_bs = found[1].expect("verified port span");
        assert!(body[name_bs.start..name_bs.end].contains("api"), "{file}");
        assert_eq!(&body[port_bs.start..port_bs.end], "8080", "{file}");
        // Editing the value invalidates its verified span.
        let edited = body.replace("8080", "9090");
        let (after_edit, _) = find_spans(edited.as_bytes(), [port_span]);
        assert_eq!(after_edit[0], None, "edited value span must invalidate");
    }

    // `entries:` on TOML places its spans without descending unrelated `[[big]]`
    // arrays (using `Holds::Value` rather than `Holds::RecordValue`).
    let mut toml_with_big = String::from("[deps]\nserde = \"1.0\"\n");
    for i in 0..20 {
        let _ = writeln!(toml_with_big, "[[big]]\nk{i} = {i}");
    }
    let files_toml = tree(&[("Cargo.toml", toml_with_big.as_str())]);
    let entries_rel = named(
        "deps",
        Rows::Part(Part {
            from: from("Cargo.toml"),
            reader: Reader::Entries(Entries {
                format: EntriesFormat::Toml,
                at: vec!["$.deps".into()],
                leaves: false,
                strict: false,
            }),
            fields: None,
        }),
        &[("k", FieldSource::EntryKey), ("v", FieldSource::EntryValue)],
    );
    let ep = plan(vec![entries_rel]);
    let rows = run(&ep, 0, &files_toml, Limits::default()).unwrap();
    let v_span = rows.rows[0].cells[1].span.as_ref().expect("entry span");
    let (found, _) = find_spans(toml_with_big.as_bytes(), [v_span]);
    let bs = found[0].expect("verified entry span");
    assert_eq!(&toml_with_big[bs.start..bs.end], "\"1.0\"");
}

#[test]
fn per_part_union_stub_rows_respect_transforms_and_where_with_differing_path_tokens() {
    use crate::relation::plan::{Member, Predicate};

    let field_def = |source: FieldSource, sensitive: bool| FieldDef { source, sensitive };
    let union_rel = NamedRelation {
        name: "locales".into(),
        source: SourceIndex(0),
        relation: RelationDef {
            rows: Rows::Union(vec![
                Part {
                    from: from("a/*.yml"),
                    reader: Reader::Entries(Entries {
                        format: EntriesFormat::Yaml,
                        at: vec!["$".into()],
                        leaves: true,
                        strict: false,
                    }),
                    fields: Some(vec![
                        ("locale".into(), field_def(FieldSource::EntryRoot, false)),
                        ("key".into(), field_def(FieldSource::EntryPath, false)),
                        (
                            "tag".into(),
                            field_def(FieldSource::Path(PathToken::Stem), false),
                        ),
                    ]),
                },
                Part {
                    from: from("b/*/*.yml"),
                    reader: Reader::Entries(Entries {
                        format: EntriesFormat::Yaml,
                        at: vec!["$".into()],
                        leaves: true,
                        strict: false,
                    }),
                    fields: Some(vec![
                        ("locale".into(), field_def(FieldSource::EntryRoot, false)),
                        ("key".into(), field_def(FieldSource::EntryPath, false)),
                        (
                            "tag".into(),
                            field_def(FieldSource::Path(PathToken::ParentName), false),
                        ),
                    ]),
                },
            ]),
            fields: vec![
                ("locale".into(), field_def(FieldSource::EntryRoot, false)),
                ("key".into(), field_def(FieldSource::EntryPath, false)),
                (
                    "tag".into(),
                    field_def(FieldSource::Path(PathToken::Stem), false),
                ),
            ],
            transforms: vec![
                ("key".into(), vec![Transform::StripRoot]),
                ("tag".into(), vec![Transform::Lower]),
            ],
            filters: vec![(
                "tag".into(),
                vec![Predicate::NotIn(vec![Member::Literal(Value::String(
                    "ignored".into(),
                ))])],
            )],
        },
        capabilities: Capabilities::default(),
        reads_environment: false,
        definition_reads_environment: false,
    };
    let files = tree(&[
        ("a/en.yml", "en:\n  hello: Hi\n"),
        ("a/IGNORED.yml", "fr: {}\n"),
        ("b/IGNORED/messages.yml", "de: {}\n"),
        ("b/kept/messages.yml", "es: {}\n"),
    ]);
    let p = plan(vec![union_rel]);
    let rows = run(&p, 0, &files, Limits::default()).unwrap();
    let stub_locales: Vec<&str> = rows
        .stub_rows
        .iter()
        .map(|s| text(&s.cells[0]).as_ref())
        .collect();
    assert_eq!(
        stub_locales,
        vec!["es"],
        "both IGNORED stubs (from stem and parent_name parts) must be transformed and filtered out"
    );
}

#[test]
fn split_clears_cell_path_and_handles_extreme_parts_and_bytes_budget() {
    // Out-of-range `split { required: false }` on `entry.path` clears `cell.path`.
    let mut rel = named(
        "keys",
        Rows::Part(Part {
            from: from("a.json"),
            reader: Reader::Entries(Entries {
                format: EntriesFormat::Json,
                at: vec!["$".into()],
                leaves: true,
                strict: false,
            }),
            fields: None,
        }),
        &[("k", FieldSource::EntryPath)],
    );
    rel.relation.transforms = vec![(
        "k".into(),
        vec![Transform::Split {
            on: ":".into(),
            part: i64::MIN,
            required: false,
        }],
    )];
    let files = tree(&[("a.json", r#"{"only": 1}"#)]);
    let p = plan(vec![rel]);
    let rows = run(&p, 0, &files, Limits::default()).unwrap();
    assert_eq!(rows.rows.len(), 1);
    assert!(rows.rows[0].cells[0].value.is_none());
    assert!(rows.rows[0].cells[0].span.is_none());
    assert!(
        rows.rows[0].cells[0].path.is_none(),
        "out-of-range split must clear stale cell.path"
    );

    // `suffix` charges output bytes against `BytesPerRun`.
    let mut budget_rel = named(
        "lines",
        Rows::Part(Part {
            from: from("a.txt"),
            reader: Reader::Read {
                read: Read::Lines { comment: None },
                optional: false,
            },
            fields: None,
        }),
        &[("v", FieldSource::Value)],
    );
    budget_rel.relation.transforms = vec![("v".into(), vec![Transform::Suffix("x".repeat(4096))])];
    let files_txt = tree(&[("a.txt", "a\nb\nc\n")]);
    let bp = plan(vec![budget_rel]);
    let errs = messages(&run(
        &bp,
        0,
        &files_txt,
        limit(LimitKind::BytesPerRun, 1024),
    ));
    assert!(
        errs.iter().any(|m| m.contains("bytes_per_run")),
        "suffix growth must charge BytesPerRun: {errs:?}"
    );
}

#[test]
fn join_propagates_matching_stub_rows_under_join_outputs_budget() {
    // Audit M2: a `join:` over a left relation with a surviving `strip-root`
    // stub row (`fr: {}`) must propagate matching stub rows so downstream
    // `set`/`count` grouped by `entry.root` still anchor at the stub file.
    let files = tree(&[
        ("locales/en.yml", "en:\n  hello: hi\n"),
        ("locales/fr.yml", "fr: {}\n"),
        ("meta.json", r#"{"en":"English","fr":"French"}"#),
    ]);
    let mut locales = named(
        "locales",
        Rows::Part(Part {
            from: from("locales/*.yml"),
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
    locales.relation.transforms = vec![("key".into(), vec![Transform::StripRoot])];
    let meta = named(
        "meta",
        Rows::Part(Part {
            from: from("meta.json"),
            reader: Reader::Entries(Entries {
                format: EntriesFormat::Json,
                at: vec!["$".into()],
                leaves: false,
                strict: false,
            }),
            fields: None,
        }),
        &[
            ("locale", FieldSource::EntryKey),
            ("label", FieldSource::EntryValue),
        ],
    );
    let joined = named(
        "joined",
        Rows::Join(crate::relation::plan::JoinDef {
            left: crate::relation::plan::Reached {
                id: RelationId(0),
                alias: None,
            },
            right: crate::relation::plan::Reached {
                id: RelationId(1),
                alias: None,
            },
            on: vec![("locale".into(), "locale".into())],
            unique: JoinSide::Right,
        }),
        &[
            ("locale", FieldSource::Side(JoinSide::Left, "locale".into())),
            ("key", FieldSource::Side(JoinSide::Left, "key".into())),
            ("label", FieldSource::Side(JoinSide::Right, "label".into())),
        ],
    );
    let p = plan(vec![locales, meta, joined]);
    let rows = run(&p, 2, &files, Limits::default()).expect("join succeeds");
    assert_eq!(rows.rows.len(), 1);
    assert_eq!(rows.stub_rows.len(), 1, "fr stub row must survive join");
    assert_eq!(
        rows.stub_rows[0].cells[0].value,
        Some(Value::String("fr".into()))
    );
    assert_eq!(
        rows.stub_rows[0].cells[2].value,
        Some(Value::String("French".into()))
    );
}

#[test]
fn join_excludes_unmatched_resources_and_detects_stub_probe_key_type_clash() {
    // 1. Inner join on path-derived key must drop unmatched resources from
    // `resources_kept` so downstream path-token `group_by` never invents a
    // phantom empty group for dropped files.
    let files = tree(&[
        ("locales/en.yml", "a: 1\n"),
        ("locales/fr.yml", "a: 1\n"),
        ("allow.txt", "en\n"),
    ]);
    let locales = named(
        "locales",
        Rows::Part(Part {
            from: from("locales/*.yml"),
            reader: Reader::Entries(Entries {
                format: EntriesFormat::Yaml,
                at: vec!["$".into()],
                leaves: true,
                strict: false,
            }),
            fields: None,
        }),
        &[
            ("stem", FieldSource::Path(PathToken::Stem)),
            ("key", FieldSource::EntryPath),
        ],
    );
    let allow = named(
        "allow",
        Rows::Part(Part {
            from: from("allow.txt"),
            reader: Reader::Read {
                read: Read::Lines { comment: None },
                optional: false,
            },
            fields: None,
        }),
        &[("stem", FieldSource::Value)],
    );
    let joined = named(
        "joined",
        Rows::Join(crate::relation::plan::JoinDef {
            left: crate::relation::plan::Reached {
                id: RelationId(0),
                alias: None,
            },
            right: crate::relation::plan::Reached {
                id: RelationId(1),
                alias: None,
            },
            on: vec![("stem".into(), "stem".into())],
            unique: JoinSide::Right,
        }),
        &[
            ("stem", FieldSource::Side(JoinSide::Left, "stem".into())),
            ("key", FieldSource::Side(JoinSide::Left, "key".into())),
        ],
    );
    let p = Arc::new(plan(vec![locales, allow, joined]));
    let index = crate::walk(files.dir.path(), &crate::WalkOptions::test_default()).unwrap();
    let ctx = crate::Context::new(files.dir.path(), &index);
    let rows = evaluate(&p, RelationId(2), &ctx).expect("join succeeds");
    assert_eq!(rows.rows.len(), 1);
    let kept = crate::relation::eval::resources_kept(&p, RelationId(2), &rows, &ctx);
    let kept_paths: Vec<&str> = rows
        .resources
        .iter()
        .zip(kept)
        .filter_map(|(r, keep)| keep.then_some(r.path().as_str()))
        .collect();
    assert_eq!(
        kept_paths,
        vec!["locales/en.yml"],
        "unmatched `fr.yml` must be filtered out by resources_kept"
    );

    // 2. A probe side holding only a stub row (`fr: {}`, string key `"fr"`)
    // joined against a numeric key (`1`) must still raise a key-type clash.
    let clash_files = tree(&[("locales/fr.yml", "fr: {}\n"), ("nums.json", r#"{"a": 1}"#)]);
    let mut stub_only = named(
        "stub_only",
        Rows::Part(Part {
            from: from("locales/*.yml"),
            reader: Reader::Entries(Entries {
                format: EntriesFormat::Yaml,
                at: vec!["$".into()],
                leaves: true,
                strict: false,
            }),
            fields: None,
        }),
        &[("k", FieldSource::EntryRoot), ("p", FieldSource::EntryPath)],
    );
    stub_only.relation.transforms = vec![("p".into(), vec![Transform::StripRoot])];
    let nums = named(
        "nums",
        Rows::Part(Part {
            from: from("nums.json"),
            reader: Reader::Entries(Entries {
                format: EntriesFormat::Json,
                at: vec!["$".into()],
                leaves: false,
                strict: false,
            }),
            fields: None,
        }),
        &[("k", FieldSource::EntryValue)],
    );
    let clash_join = named(
        "clash",
        Rows::Join(crate::relation::plan::JoinDef {
            left: crate::relation::plan::Reached {
                id: RelationId(0),
                alias: None,
            },
            right: crate::relation::plan::Reached {
                id: RelationId(1),
                alias: None,
            },
            on: vec![("k".into(), "k".into())],
            unique: JoinSide::Right,
        }),
        &[("k", FieldSource::Side(JoinSide::Left, "k".into()))],
    );
    let p_clash = plan(vec![stub_only, nums, clash_join]);
    let errs = messages(&run(&p_clash, 2, &clash_files, Limits::default()));
    assert!(
        errs.iter()
            .any(|m| m.contains("join keys compare within one type")),
        "stub-only probe key type clash must fail loudly: {errs:?}"
    );
}
